// SPDX-License-Identifier: LGPL-3.0-or-later
//! Combines `p25_ratet27_probe_chanfmt_ecmode.rs`'s ground-truth `ECMODE_OUT` status flag readout
//! with `p25_ratet27_capture_dtx_noise_levels.rs`'s noise-level sweep, to get an unambiguous read
//! of the chip's own `VOICE_ACTIVE` classification (rather than inferring the silence/voice
//! boundary from `g0`/`g2`/`c7`'s own wire values, as section 26 had to). With `DTX_ENABLE` on,
//! DVSI's manual states the encoder sets `VOICE_ACTIVE=0` for frames that don't need transmitting
//! (confirmed silence) and `=1` otherwise -- ground truth for exactly where that boundary falls
//! relative to the noise peaks already tested, and whether `g3` correlates with it.
//!
//! Usage: `cargo run --release --example p25_ratet27_probe_dtx_voice_active -- <host:port>`
use ham_digital_modes::ambe::float::general::fec::golay_decode;
use ham_digital_modes::ambe::float::ratet27::ratet27_fec::decode_block;
use ham_digital_modes::ambe::float::ratet27::ratet27_wire_format::{block_wire_members, Block};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const FIELD_CHANFMT: u8 = 0x15;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const DTX_ENABLE_BIT: u16 = 1 << 11;
const TD_ENABLE_BIT: u16 = 1 << 12;
const SETTLING_FRAMES: usize = 80;
const CAPTURE_FRAMES: usize = 10;
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
fn build_control_ecmode(ecmode_in: u16) -> Vec<u8> {
    let mut payload = vec![FIELD_ECMODE];
    payload.extend_from_slice(&ecmode_in.to_be_bytes());
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
fn build_control_chanfmt(data: u16) -> Vec<u8> {
    let mut payload = vec![FIELD_CHANFMT];
    payload.extend_from_slice(&data.to_be_bytes());
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
fn lcg_noise(seed: u64, peak: f64) -> Vec<i16> {
    let mut state = seed;
    (0..FRAME_SAMPLES)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let unit = ((state >> 33) as f64 / (1u64 << 31) as f64) - 1.0;
            (unit * peak) as i16
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
    parse_packet(&buf[..n]).expect("valid packet");

    sock.send(&build_control_ecmode(DTX_ENABLE_BIT | TD_ENABLE_BIT)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");

    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

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

    let noise_peaks: [f64; 10] = [0.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 75.0, 100.0, 150.0];

    for &peak in &noise_peaks {
        let samples = if peak == 0.0 { vec![0i16; FRAME_SAMPLES] } else { lcg_noise(42, peak) };
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            assert_eq!(payload[1] as usize, TOTAL_BITS);
            let pkt = &buf[..n];
            let ecmode_out = u16::from_be_bytes([pkt[n - 2], pkt[n - 1]]);
            let voice_active = (ecmode_out >> 1) & 1;
            let bits_bytes = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let mut wire_frame_bits = [false; TOTAL_BITS];
            for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
                for bit_idx in 0..8 {
                    wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
                }
            }
            let (g0, _) = decode_block(&wire_frame_bits, Block::Golay { index: 0 });
            // g3's real generator isn't confirmed (decode_block refuses it), but every session-long
            // observation of g3 has used the standard Golay code as a working assumption -- do the
            // same here, purely to observe whether it's constant/varies, not as a validated decode.
            let g3_members = block_wire_members(Block::Golay { index: 3 });
            let mut g3_received: u32 = 0;
            for (offset, &wire) in g3_members.iter().enumerate() {
                if wire_frame_bits[wire] {
                    g3_received |= 1 << (g3_members.len() - 1 - offset);
                }
            }
            let (g3, _) = golay_decode(g3_received);
            println!(
                "peak={peak:6.1} frame={i} VOICE_ACTIVE={voice_active} g0={g0:5} g3={g3:5}"
            );
        }
    }
}
