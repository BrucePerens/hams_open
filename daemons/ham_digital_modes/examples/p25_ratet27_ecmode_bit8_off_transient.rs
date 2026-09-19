// SPDX-License-Identifier: LGPL-3.0-or-later
//! Directly tests the contamination *mechanism* proposed (not yet confirmed) in section 38's own
//! correction: that the original bit-8 amplitude probe's contaminated "baseline" readings (e.g.
//! `1049` at amplitude 138.2, vs. the properly-settled `1561`) resulted from insufficient settling
//! (60 frames) after switching `ECMODE_IN` from `1<<8` back to `0` at the same amplitude. Settles at
//! amplitude 138.2 with bit 8 ON (expect `g0=1597`, per section 38), then switches `ECMODE_IN` to
//! `0x0000` with NO change to the signal and logs `g0` for 120 frames. If the decay takes more than
//! ~60 frames to reach `1561`, that confirms the settling-shortfall mechanism directly. If it snaps
//! back in 1-2 frames, the mechanism is wrong and the original `1049` reading needs a different
//! explanation. Also watches for undershoot below `1561` (the "contrast with adapted baseline"
//! sub-hypothesis) versus a monotonic, non-overshooting decay.
//!
//! Usage: `cargo run --release --example p25_ratet27_ecmode_bit8_off_transient -- <host:port>`
use ham_digital_modes::ambe::dvsi_p25fec::frame::decode_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 300;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const ECMODE_BIT8: u16 = 1 << 8;
const TEST_AMPLITUDE: f64 = 138.2;
const TEST_FREQ: f64 = 200.0;

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
fn sawtooth(freq: f64, amp: f64) -> Vec<i16> {
    let period = 8000.0 / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            (amp * (2.0 * phase - 1.0)) as i16
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

    let samples = sawtooth(TEST_FREQ, TEST_AMPLITUDE);

    sock.send(&build_control_ecmode(ECMODE_BIT8)).expect("send ECMODE config (bit 8 on)");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");

    println!("-- Settling {SETTLING_FRAMES} frames at amplitude {TEST_AMPLITUDE}, bit 8 ON --");
    for _ in 0..SETTLING_FRAMES {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
        parse_packet(&buf[..n]).expect("valid packet");
    }
    for i in 0..3 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let pkt = &buf[..n];
        let bits_bytes: &[u8; FRAME_BYTES] = pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES].try_into().unwrap();
        let frame = decode_frame(bits_bytes);
        println!("  settled bit8-on frame {i}: g0={}", frame.g0.value);
    }

    println!("\n-- Switching ECMODE_IN to 0x0000, NO change to signal, logging g0 for 120 frames --");
    sock.send(&build_control_ecmode(0x0000)).expect("send ECMODE config (off)");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    for i in 0..120 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let pkt = &buf[..n];
        let bits_bytes: &[u8; FRAME_BYTES] = pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES].try_into().unwrap();
        let frame = decode_frame(bits_bytes);
        println!("  off frame {i}: g0={}", frame.g0.value);
    }
}
