// SPDX-License-Identifier: LGPL-3.0-or-later
//! The first real PASS/FAIL chip-validation harness for RATET(27) (P25 full-rate with FEC), in the
//! same style as `ambe_chip_validate_dstar.rs` and `ambe_chip_validate_ambe_plus_2.rs`: captures
//! real frames from the live chip across several frequencies and checks that this crate's own
//! `ambe::ratet27_wire_format`/`ambe::ratet27_fec` modules -- the real wire format and FEC codes
//! this investigation determined by direct chip-frame sampling, not assumed or guessed -- decode
//! every one of them with **zero corrected errors** on the 7 sub-blocks confirmed this session
//! (`g0`, `g1`, `g2`, `u4`, `u5`, `u6`, `c7`; `g3` is deliberately excluded -- its real generator
//! matrix is not yet confirmed, see `AMBE_CHIP_VALIDATION_FINDINGS.md` section 23).
//!
//! Unlike the older, now-superseded `ambe_chip_validate_p25_wireformat.rs` (a *search* harness that
//! assumed a content-dependent PRN whitening stage this session's GF(2) rank analysis has since
//! shown does not exist for RATET(27) -- 7 of 8 blocks reached full rank, meaning the wire bits are
//! genuinely unwhitened codewords), this harness has no search or hypothesis-scoring step at all: it
//! decodes directly via the already-determined-correct format and simply reports whether every
//! frame's every block was a valid, error-free codeword -- the same "does the real chip's own output
//! match what this crate's own software says a valid codeword should look like" bar the D-STAR and
//! AMBE+2 harnesses already apply.
//!
//! Usage: `cargo run --release --example ambe_chip_validate_ratet27 -- <host:port>`
use ham_digital_modes::ambe::ratet27_fec::decode_block;
use ham_digital_modes::ambe::ratet27_wire_format::Block;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 40;
const CAPTURE_FRAMES: usize = 15;
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
fn sawtooth(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            (6000.0 * (2.0 * phase - 1.0)) as i16
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

    let n = {
        sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
        sock.recv(&mut buf).expect("RATEP config response")
    };
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
    println!("RATEP config response: type={ptype:02x} payload={payload:02x?}");

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

    let blocks_to_check = [
        Block::Golay { index: 0 },
        Block::Golay { index: 1 },
        Block::Golay { index: 2 },
        Block::Hamming { index: 0 },
        Block::Hamming { index: 1 },
        Block::Hamming { index: 2 },
        Block::Raw,
    ];
    let block_names = ["g0", "g1", "g2", "u4", "u5", "u6", "c7"];

    let frequencies = [50.0, 100.0, 200.0, 250.0, 400.0, 500.0, 800.0, 1000.0];
    let mut total_frames = 0usize;
    let mut total_zero_error_frames = 0usize;

    for &freq in &frequencies {
        let samples = sawtooth(freq);
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        let mut zero_error_this_freq = 0;
        for _ in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
            assert_eq!(payload[1] as usize, TOTAL_BITS, "unexpected bit count");

            let pkt = &buf[..n];
            let bits_bytes = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let mut wire_frame_bits = [false; TOTAL_BITS];
            for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
                for bit_idx in 0..8 {
                    wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
                }
            }

            let mut all_zero_error = true;
            for &block in &blocks_to_check {
                let (_data, distance) = decode_block(&wire_frame_bits, block);
                if distance != 0 {
                    all_zero_error = false;
                }
            }
            total_frames += 1;
            if all_zero_error {
                total_zero_error_frames += 1;
                zero_error_this_freq += 1;
            }
        }
        println!(
            "  {freq:6.0}Hz: {zero_error_this_freq}/{CAPTURE_FRAMES} frames zero-error on all 7 confirmed blocks ({})",
            block_names.join(",")
        );
    }

    println!();
    if total_zero_error_frames == total_frames {
        println!(
            "PASS: all {total_frames} captured frames across {} frequencies decoded with zero errors on every confirmed block (g0,g1,g2,u4,u5,u6,c7).",
            frequencies.len()
        );
    } else {
        println!(
            "FAIL: only {total_zero_error_frames}/{total_frames} frames decoded with zero errors on every confirmed block."
        );
        std::process::exit(1);
    }
}
