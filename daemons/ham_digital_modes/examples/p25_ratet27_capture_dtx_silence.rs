// SPDX-License-Identifier: LGPL-3.0-or-later
//! Tests whether `DTX_ENABLE` (`ECMODE_IN` bit 11, Discontinuous Transmission / Voice Activity
//! Detection) changes RATET(27)'s wire format for silence, following the same primary-source-
//! motivated pattern that found the real DTMF encoding mode
//! (`p25_ratet27_capture_real_dtmf.rs`/`ambe::ratet27_dtmf`). DVSI's manual: with VAD enabled, "the
//! encoder will output a silence frame (in-band)... [which] contains information regarding the
//! level of background noise" -- a specific, testable claim that silence frames under `DTX_ENABLE`
//! carry a real noise-level parameter, distinct from ordinary silence handling.
//!
//! Captures pure digital silence with `DTX_ENABLE` off (default) and on, plus a loud tone as a
//! control (to confirm `DTX_ENABLE` doesn't also change voiced-frame encoding), for offline
//! comparison.
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_dtx_silence -- <host:port>`
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 60;
const CAPTURE_FRAMES: usize = 10;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const DTX_ENABLE_BIT: u16 = 1 << 11;

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
fn silence() -> Vec<i16> {
    vec![0i16; FRAME_SAMPLES]
}
fn sawtooth(freq: f64, amp: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            (amp * (2.0 * phase - 1.0)) as i16
        })
        .collect()
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

    let capture = |sock: &UdpSocket, buf: &mut [u8; 512], label: &str, samples: &[i16]| {
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
            println!("{label}\t{i}\t{hex}");
        }
    };

    // DTX off (default: ECMODE_IN = 0x0000)
    sock.send(&build_control_ecmode(0x0000)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    capture(&sock, &mut buf, "dtxoff_silence", &silence());
    capture(&sock, &mut buf, "dtxoff_tone", &sawtooth(200.0, 6000.0));
    capture(&sock, &mut buf, "dtxoff_noise1", &lcg_noise(1, 3000.0));
    capture(&sock, &mut buf, "dtxoff_noise2", &lcg_noise(2, 1000.0));

    // DTX on
    sock.send(&build_control_ecmode(DTX_ENABLE_BIT)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    capture(&sock, &mut buf, "dtxon_silence", &silence());
    capture(&sock, &mut buf, "dtxon_tone", &sawtooth(200.0, 6000.0));
    capture(&sock, &mut buf, "dtxon_noise1", &lcg_noise(1, 3000.0));
    capture(&sock, &mut buf, "dtxon_noise2", &lcg_noise(2, 1000.0));
    capture(&sock, &mut buf, "dtxon_lowlevelnoise", &lcg_noise(3, 200.0));
}
