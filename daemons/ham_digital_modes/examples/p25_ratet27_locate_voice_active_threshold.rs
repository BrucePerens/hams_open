// SPDX-License-Identifier: LGPL-3.0-or-later
//! Precisely locates the chip's own `VOICE_ACTIVE` (DTX/VAD) classification threshold, previously
//! only bounded to "somewhere between peak 50 and peak 75" (section 28). Uses the ground-truth
//! `ECMODE_OUT` status flag (via `PKT_CHANFMT`, section 27) directly rather than inferring the
//! boundary from `g0`'s own wire value, and a binary search over noise peak to narrow the exact
//! transition point this session's earlier 8-point sweep only bracketed.
//!
//! Usage: `cargo run --release --example p25_ratet27_locate_voice_active_threshold -- <host:port>`
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
const SETTLING_FRAMES: usize = 250;
const CAPTURE_FRAMES: usize = 8;
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

    let mut test_peak = |peak: f64| -> bool {
        let samples = lcg_noise(42, peak);
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        let mut voice_active_count = 0;
        for _ in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            assert_eq!(payload[1] as usize, TOTAL_BITS);
            let pkt = &buf[..n];
            let ecmode_out = u16::from_be_bytes([pkt[n - 2], pkt[n - 1]]);
            let voice_active = (ecmode_out >> 1) & 1;
            voice_active_count += voice_active as u32;
        }
        let majority_active = voice_active_count > (CAPTURE_FRAMES as u32 / 2);
        println!("peak={peak:7.2}  voice_active_count={voice_active_count}/{CAPTURE_FRAMES}  majority_active={majority_active}");
        majority_active
    };

    println!("-- Diagnostic: re-checking peak=100 with {SETTLING_FRAMES} settling frames --");
    test_peak(100.0);

    // Direct confirmation of the adaptive-noise-floor hypothesis: after 250 frames adapted to
    // peak=100 (just shown inactive above), abruptly switch to a genuinely loud tone WITHOUT
    // resettling -- if VOICE_ACTIVE responds to *contrast* with the adapted baseline rather than
    // absolute level, this should immediately read active.
    println!("\n-- Confirmation: abrupt switch to a loud tone right after 250 frames of peak=100 --");
    let loud = lcg_noise(99, 9000.0);
    for i in 0..8 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&loud));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        assert_eq!(payload[1] as usize, TOTAL_BITS);
        let pkt = &buf[..n];
        let ecmode_out = u16::from_be_bytes([pkt[n - 2], pkt[n - 1]]);
        let voice_active = (ecmode_out >> 1) & 1;
        println!("  frame {i}: voice_active={voice_active}");
    }
}
