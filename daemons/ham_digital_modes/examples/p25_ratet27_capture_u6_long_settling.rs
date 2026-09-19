// SPDX-License-Identifier: LGPL-3.0-or-later
//! Direct follow-up to section 32's adaptive-baseline discovery: tests whether `u6`'s instability
//! at 600Hz (15 distinct values across 20 frames with 60 settling frames, section 30's retraction)
//! resolves with a much longer, more patient settling period, the same way `VOICE_ACTIVE` did once
//! given 250 frames instead of 40-80.
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_u6_long_settling -- <host:port>`
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const TARGET_RMS: f64 = 3464.0;

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
fn rms_normalized_sawtooth(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    let raw: Vec<f64> = (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            2.0 * phase - 1.0
        })
        .collect();
    let rms: f64 = (raw.iter().map(|&x| x * x).sum::<f64>() / raw.len() as f64).sqrt();
    let scale = TARGET_RMS / rms;
    raw.iter().map(|&x| (x * scale).clamp(-32000.0, 32000.0) as i16).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let freq: f64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(600.0);
    let settling: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(500);

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
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

    let samples = rms_normalized_sawtooth(freq);
    eprintln!("Settling {settling} frames at {freq}Hz...");
    for i in 0..settling {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
        parse_packet(&buf[..n]).expect("valid packet");
        if i % 100 == 0 {
            eprintln!("  ...{i}/{settling}");
        }
    }
    eprintln!("Capturing 20 frames...");
    for i in 0..20 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let pkt = &buf[..n];
        let bits = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
        let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
        println!("u6long_{freq}_{settling}\t{i}\t{hex}");
    }
}
