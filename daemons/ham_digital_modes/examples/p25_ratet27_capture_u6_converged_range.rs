// SPDX-License-Identifier: LGPL-3.0-or-later
//! Focused follow-up to the `u6` pitch-candidate correction (section 30): `u6` was found highly
//! unstable below ~280Hz (5-8 distinct values across 8 frames) but frame-stable at 340Hz and above.
//! Rather than force convergence at low frequencies (a known, accepted chip limitation this
//! investigation has documented repeatedly, not something more settling frames fixes), this tool
//! restricts testing to the range where `u6` already showed genuine stability -- 280Hz through
//! 1000Hz (extending past AMBE's own designed range, matching the D-STAR/AMBE+2 harnesses' own
//! convention of testing above-range frequencies too) -- with more captured frames per point (20)
//! to get a statistically solid reading of `u6`'s real behavior where it's actually reliable.
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_u6_converged_range -- <host:port>`
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
const CAPTURE_FRAMES: usize = 20;
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

    let frequencies: [f64; 15] = [
        280.0, 300.0, 320.0, 340.0, 360.0, 380.0, 400.0, 420.0, 440.0, 500.0, 600.0, 700.0, 800.0,
        900.0, 1000.0,
    ];

    for &freq in &frequencies {
        let samples = rms_normalized_sawtooth(freq);
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
            println!("u6conv_{freq}\t{i}\t{hex}");
        }
    }
}
