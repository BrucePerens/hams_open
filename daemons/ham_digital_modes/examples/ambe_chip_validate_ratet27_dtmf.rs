// SPDX-License-Identifier: LGPL-3.0-or-later
//! Live chip validation for `ambe::dvsi_p25fec::dtmf`: feeds all 16 real ITU-T Q.23 DTMF digit tones to
//! the chip and confirms `decode_dtmf_digit` correctly recovers the exact row/column pair for
//! every one, straight from the chip's own live response (not the frozen capture the module's own
//! unit tests check against).
//!
//! Usage: `cargo run --release --example ambe_chip_validate_ratet27_dtmf -- <host:port>`
use ham_digital_modes::ambe::dvsi_p25fec::dtmf::decode_dtmf_digit;
use ham_digital_modes::ambe::dvsi_p25fec::fec::decode_block;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::Block;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 60;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const TOTAL_BITS: usize = 144;

fn build_control_ratep(rcw: [u16; 6]) -> Vec<u8> {
    let mut payload = vec![FIELD_RATEP];
    for v in rcw {
        payload.extend_from_slice(&v.to_be_bytes());
    }
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
fn build_speech(samples: &[i16]) -> Vec<u8> {
    let mut payload = vec![0x00_u8, samples.len() as u8];
    for &s in samples {
        payload.extend_from_slice(&s.to_be_bytes());
    }
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_SPEECH);
    pkt.extend_from_slice(&payload);
    pkt
}
fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    data.get(4..4 + length).map(|payload| (ptype, payload))
}
fn dtmf_tone(row_hz: f64, col_hz: f64, amp: f64) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|n| {
            let t = n as f64 / SAMPLE_RATE;
            let s = (2.0 * std::f64::consts::PI * row_hz * t).sin()
                + (2.0 * std::f64::consts::PI * col_hz * t).sin();
            (amp * 0.5 * s) as i16
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid DVSI packet");

    let send_recv_retrying = |sock: &UdpSocket, buf: &mut [u8; 512], pkt: &[u8]| -> usize {
        for attempt in 0..8 {
            sock.send(pkt).expect("send");
            match sock.recv(buf) {
                Ok(n) => return n,
                Err(e) if attempt < 7 => {
                    eprintln!("retrying after {e}");
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => panic!("recv channel after retries: {e}"),
            }
        }
        unreachable!()
    };

    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0, 1633.0];
    let digit_names = [["1", "2", "3", "A"], ["4", "5", "6", "B"], ["7", "8", "9", "C"], ["*", "0", "#", "D"]];

    let mut all_ok = true;
    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
            for _ in 0..SETTLING_FRAMES {
                let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
                parse_packet(&buf[..n]).expect("valid packet");
            }
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            assert_eq!(payload[1] as usize, TOTAL_BITS);
            let pkt = &buf[..n];
            let bits_bytes = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let mut wire_frame_bits = [false; TOTAL_BITS];
            for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
                for bit_idx in 0..8 {
                    wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
                }
            }
            let (g0, _) = decode_block(&wire_frame_bits, Block::Golay { index: 0 });
            let (u4, _) = decode_block(&wire_frame_bits, Block::Hamming { index: 0 });
            let decoded = decode_dtmf_digit(g0, u4);
            let ok = decoded == Some((ri as u8, ci as u8));
            if !ok {
                all_ok = false;
            }
            println!(
                "  digit {}: g0={g0} u4={u4} decoded={decoded:?} expected=Some(({ri},{ci})) {}",
                digit_names[ri][ci],
                if ok { "OK" } else { "MISMATCH" }
            );
        }
    }

    if all_ok {
        println!("\nPASS: all 16 DTMF digits decoded correctly via ambe::dvsi_p25fec::dtmf::decode_dtmf_digit.");
    } else {
        eprintln!("\nFAIL: at least one DTMF digit did not decode correctly.");
        std::process::exit(1);
    }
}
