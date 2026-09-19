// SPDX-License-Identifier: LGPL-3.0-or-later
//! Investigates a real discrepancy caught by direct comparison, not assumed away: `examples/
//! p25_ratet27_ecmode_bit8_amplitude_probe.rs`'s explicit `ECMODE_IN=0x0000` baseline reads `g0` far
//! LOWER at low-to-mid amplitudes (e.g. `1049` at amplitude 138.2) than the already-committed
//! `g0_long_settling_amplitude_sweep.tsv` (`1561` at the same amplitude, section 23/33), which never
//! sent any `PKT_ECMODE` control packet at all and so captured whatever the chip's power-on-default
//! `ECMODE_IN` actually is. Section 25 already established `TD_ENABLE` (on by default) doesn't
//! explain this by itself (ruled out at one high-amplitude test point) -- but that test was never
//! run across the low-amplitude range where this discrepancy actually shows up, so it doesn't rule
//! out an amplitude-dependent default-on feature (plausibly bit 8 itself, or a different one).
//!
//! This tool captures `g0` at one fixed, low amplitude (200Hz, amplitude 138.2, matching the
//! discrepant point exactly) under three conditions back to back: (1) `ECMODE_IN` never touched at
//! all (true power-on-reset default, closest to how the original sweep was captured), (2)
//! `ECMODE_IN` explicitly set to `0x0000` (every feature off), (3) `ECMODE_IN` explicitly set to
//! `1<<8` (bit 8 forced on). If (1) and (3) agree while (2) differs, that's direct, decisive evidence
//! bit 8 (or something with the same effect) is on by default.
//!
//! Usage: `cargo run --release --example p25_ratet27_ecmode_default_state_probe -- <host:port>`
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
const CAPTURE_FRAMES: usize = 8;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
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

    // Freshly connects and sends only RATEP first -- this is condition (1): ECMODE_IN is never
    // touched, exactly matching how p25_ratet27_capture_g0_long_settling_amplitude.rs (and the
    // original section 23 sweep) captured their own data, so this reproduces the same starting
    // state rather than assuming what it was.
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
    let capture = |sock: &UdpSocket, buf: &mut [u8; 512], label: &str| {
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        let mut g0_vals = Vec::with_capacity(CAPTURE_FRAMES);
        let mut u4_vals = Vec::with_capacity(CAPTURE_FRAMES);
        for _ in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(&samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits_bytes: &[u8; FRAME_BYTES] =
                pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES].try_into().unwrap();
            let frame = decode_frame(bits_bytes);
            g0_vals.push(frame.g0.value);
            u4_vals.push(frame.u4.value);
        }
        println!("{label}: g0={g0_vals:?} u4={u4_vals:?}");
    };

    println!("-- Condition 1: ECMODE_IN never touched (true power-on/reset default) --");
    capture(&sock, &mut buf, "untouched_default");

    println!("\n-- Condition 2: ECMODE_IN explicitly set to 0x0000 --");
    sock.send(&build_control_ecmode(0x0000)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    capture(&sock, &mut buf, "explicit_zero");

    println!("\n-- Condition 3: ECMODE_IN explicitly set to 1<<8 (bit 8 forced on) --");
    sock.send(&build_control_ecmode(1 << 8)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    capture(&sock, &mut buf, "bit8_forced_on");

    // Reset to a known-clean state for whatever uses the chip next.
    sock.send(&build_control_ecmode(0x0000)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
}
