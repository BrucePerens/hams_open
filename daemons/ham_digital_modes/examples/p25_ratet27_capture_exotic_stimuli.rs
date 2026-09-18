// SPDX-License-Identifier: LGPL-3.0-or-later
//! Third follow-up to `p25_ratet27_capture_frames.rs`: qualitatively different stimuli from
//! anything tried so far (dual-tone/DTMF-style two-sinusoid mixes, fast intra-frame chirps, and
//! extreme-pitch tones outside AMBE's normal 57-444Hz range) -- a last attempt to move the `g3`
//! Golay block off its stubborn GF(2) rank-8 plateau (see `p25_ratet27_capture_frames.rs`'s own
//! module doc; ~2500 distinct frames of tones/noise/real speech all landed in the same 8-dimensional
//! subspace).
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_exotic_stimuli -- <host:port>`
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 15;
const CAPTURE_FRAMES: usize = 15;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;

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
fn dual_tone(f1: f64, f2: f64, amp: f64) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|n| {
            let t = n as f64 / SAMPLE_RATE;
            let s = (2.0 * std::f64::consts::PI * f1 * t).sin() + (2.0 * std::f64::consts::PI * f2 * t).sin();
            (amp * 0.5 * s) as i16
        })
        .collect()
}
/// Linear chirp within one 20ms frame, from f0 to f1 Hz.
fn chirp(f0: f64, f1: f64, amp: f64) -> Vec<i16> {
    let duration = FRAME_SAMPLES as f64 / SAMPLE_RATE;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let t = n as f64 / SAMPLE_RATE;
            let k = (f1 - f0) / duration;
            let phase = 2.0 * std::f64::consts::PI * (f0 * t + 0.5 * k * t * t);
            (amp * phase.sin()) as i16
        })
        .collect()
}
fn sine(freq: f64, amp: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (amp * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
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
    let capture_frame = |sock: &UdpSocket, buf: &mut [u8; 512], label: &str, idx: usize, samples: &[i16]| {
        let n = send_recv_retrying(sock, buf, &build_speech(samples));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let pkt = &buf[..n];
        let bits = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
        let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
        println!("{label}\t{idx}\t{hex}");
    };

    let mut stimuli: Vec<(String, Vec<i16>)> = vec![];
    for &(f1, f2) in &[(60.0, 90.0), (100.0, 250.0), (150.0, 380.0), (57.0, 444.0), (200.0, 210.0)] {
        stimuli.push((format!("dualtone_{f1}_{f2}"), dual_tone(f1, f2, 5000.0)));
    }
    for &(f0, f1) in &[(57.0, 444.0), (444.0, 57.0), (100.0, 400.0), (400.0, 100.0), (57.0, 200.0)] {
        stimuli.push((format!("chirp_{f0}_{f1}"), chirp(f0, f1, 7000.0)));
    }
    // Extreme pitch, outside/at the edges of AMBE's own designed 57-444Hz range.
    for &freq in &[30.0, 40.0, 57.0, 444.0, 500.0, 600.0, 800.0] {
        stimuli.push((format!("extreme_sine_{freq}"), sine(freq, 7000.0)));
    }

    for (label, samples) in &stimuli {
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            capture_frame(&sock, &mut buf, label, i, samples);
        }
    }
}
