// SPDX-License-Identifier: LGPL-3.0-or-later
//! Discriminates AGC (adaptive, converges over some time constant) from a static compander (instant,
//! permanent shift) for `ECMODE_IN` bit 8, per the temporal test named as the real discriminator
//! after the amplitude-sweep probe's baseline column turned out to be contaminated by cross-
//! amplitude adaptive carryover (not a clean per-amplitude measurement -- see
//! `AMBE_CHIP_VALIDATION_FINDINGS.md` section 38's own correction). Holds bit 8 on throughout, settles
//! 250 frames at a quiet amplitude, then abruptly switches to a much louder tone with NO resettling
//! (the same abrupt-switch protocol `p25_ratet27_locate_voice_active_threshold.rs` used to confirm
//! `VOICE_ACTIVE`'s own adaptive behavior in section 32), logging `g0` every frame around the switch.
//! A multi-frame transient before settling supports AGC; an instant jump supports a static transform.
//!
//! **Superseded framing, kept for the historical record**: bit 8 is now confirmed as `CP_ENABLE`
//! (Compand Enable) direct from DVSI's own manual (section 39), and it doesn't apply a gain curve at
//! all -- it tells the chip the incoming samples' *format* (linear vs. µ-law/A-law). This tool always
//! sent linear PCM, so with bit 8 set the chip was misinterpreting those samples as µ-law bytes, not
//! adjusting gain on a correctly-understood signal. There was never an AGC (or any gain control) for
//! this test to discriminate against a compander -- both the "quiet" and "loud" tones in this test
//! were genuinely different, equally-mismatched inputs, not a real loudness change the chip was
//! normalizing. The result (no transient) still stands as a real, correctly-measured observation; the
//! AGC-vs-compander question it was designed to answer turned out to be the wrong question.
//!
//! Usage: `cargo run --release --example p25_ratet27_ecmode_bit8_abrupt_switch -- <host:port>`
use ham_digital_modes::ambe::float::ratet27::ratet27_frame::decode_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 250;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const ECMODE_BIT8: u16 = 1 << 8;

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
    sock.send(&build_control_ecmode(ECMODE_BIT8)).expect("send ECMODE config (bit 8 on)");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
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

    let quiet = sawtooth(200.0, 100.0);
    println!("-- Settling {SETTLING_FRAMES} frames at quiet amplitude 100.0, bit 8 held on --");
    for _ in 0..SETTLING_FRAMES {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&quiet));
        parse_packet(&buf[..n]).expect("valid packet");
    }
    for i in 0..5 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&quiet));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let pkt = &buf[..n];
        let bits_bytes: &[u8; FRAME_BYTES] = pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES].try_into().unwrap();
        let frame = decode_frame(bits_bytes);
        println!("  settled quiet frame {i}: g0={}", frame.g0.value);
    }

    println!("\n-- Abrupt switch to loud amplitude 9000.0, NO resettling, bit 8 still on --");
    let loud = sawtooth(200.0, 9000.0);
    for i in 0..20 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&loud));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let pkt = &buf[..n];
        let bits_bytes: &[u8; FRAME_BYTES] = pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES].try_into().unwrap();
        let frame = decode_frame(bits_bytes);
        println!("  switch frame {i}: g0={}", frame.g0.value);
    }

    // Reset to a known-clean state.
    sock.send(&build_control_ecmode(0x0000)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
}
