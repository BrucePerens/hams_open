// SPDX-License-Identifier: LGPL-3.0-or-later
//! Closes two gaps the advisor flagged in `ratet27_dtx`'s own module doc: (1) the module's
//! validation so far only infers correctness from "we sent silence" / "we sent a loud tone", the
//! same stimulus-inference weakness DTMF had before `PKT_CHANFMT`'s `ECMODE_OUT` gave it real
//! chip-reported ground truth; (2) whether `DTX_SILENCE_G0`'s classification is itself
//! adaptive/history-dependent the way section 32 found `VOICE_ACTIVE` to be, never tested.
//!
//! Reads `g0` and the chip's own `VOICE_ACTIVE` status flag (via `ECMODE_OUT`) together, in two
//! passes:
//! (a) across `p25_ratet27_locate_voice_active_threshold`'s own noise-peak sweep, to see whether
//!     `g0 == 3841` and `VOICE_ACTIVE == 0` agree on every single frame (real ground-truth
//!     validation of the classifier, not stimulus inference);
//! (b) repeating that tool's abrupt-switch protocol (250 settling frames at peak=100, previously
//!     shown `VOICE_ACTIVE`-inactive, then an abrupt switch to a genuinely loud, unrelated tone with
//!     NO resettling) while also watching `g0`, to see whether `g0`'s own classification tracks
//!     `VOICE_ACTIVE`'s adaptive/contrast-based behavior frame-for-frame, or lags/differs from it.
//!
//! Usage: `cargo run --release --example p25_ratet27_dtx_ground_truth_and_adaptive_check -- <host:port>`
use ham_digital_modes::ambe::ratet27_fec::decode_block;
use ham_digital_modes::ambe::ratet27_wire_format::Block;
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
const CAPTURE_FRAMES: usize = 10;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const TOTAL_BITS: usize = 144;
const DTX_SILENCE_G0: u16 = 3841;

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
fn read_g0_and_voice_active(buf: &[u8], n: usize) -> (u16, u16) {
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
    assert_eq!(ptype, TYPE_CHANNEL);
    assert_eq!(payload[1] as usize, TOTAL_BITS);
    let ecmode_out = u16::from_be_bytes([buf[n - 2], buf[n - 1]]);
    let voice_active = (ecmode_out >> 1) & 1;
    let bits_bytes = &buf[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
    let mut wire_frame_bits = [false; TOTAL_BITS];
    for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
        for bit_idx in 0..8 {
            wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
        }
    }
    let (g0, _) = decode_block(&wire_frame_bits, Block::Golay { index: 0 });
    (g0, voice_active)
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

    println!("-- Pass (a): g0 vs VOICE_ACTIVE ground truth across the noise-peak sweep --");
    let noise_peaks: [f64; 8] = [0.0, 5.0, 10.0, 20.0, 30.0, 50.0, 75.0, 100.0];
    let mut agree = 0usize;
    let mut disagree = 0usize;
    for &peak in &noise_peaks {
        let samples = if peak == 0.0 { vec![0i16; FRAME_SAMPLES] } else { lcg_noise(42, peak) };
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            let (g0, voice_active) = read_g0_and_voice_active(&buf, n);
            let g0_says_silence = g0 == DTX_SILENCE_G0;
            let voice_says_inactive = voice_active == 0;
            let matches = g0_says_silence == voice_says_inactive;
            if matches {
                agree += 1;
            } else {
                disagree += 1;
            }
            println!(
                "peak={peak:6.1} frame={i} g0={g0:5} g0_says_silence={g0_says_silence} VOICE_ACTIVE={voice_active} agree={matches}"
            );
        }
    }
    println!("\nPass (a) summary: agree={agree} disagree={disagree}");

    println!("\n-- Pass (b): abrupt switch to a loud tone right after 250 frames of peak=100 --");
    let moderate = lcg_noise(42, 100.0);
    for _ in 0..SETTLING_FRAMES {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&moderate));
        parse_packet(&buf[..n]).expect("valid packet");
    }
    for i in 0..CAPTURE_FRAMES {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&moderate));
        let (g0, voice_active) = read_g0_and_voice_active(&buf, n);
        println!("  settled frame {i}: g0={g0:5} g0_says_silence={} VOICE_ACTIVE={voice_active}", g0 == DTX_SILENCE_G0);
    }
    let loud = lcg_noise(99, 9000.0);
    for i in 0..8 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&loud));
        let (g0, voice_active) = read_g0_and_voice_active(&buf, n);
        println!("  switch frame {i}: g0={g0:5} g0_says_silence={} VOICE_ACTIVE={voice_active}", g0 == DTX_SILENCE_G0);
    }

    println!("\n-- Pass (c): does a very long (600-frame) sustained moderate signal ever settle to g0=3841? --");
    let mut silence_seen_at: Option<usize> = None;
    for i in 0..600 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&moderate));
        let (g0, voice_active) = read_g0_and_voice_active(&buf, n);
        if g0 == DTX_SILENCE_G0 && silence_seen_at.is_none() {
            silence_seen_at = Some(i);
            println!("  g0 first read DTX_SILENCE_G0 (3841) at frame {i}, VOICE_ACTIVE={voice_active}");
        }
    }
    if silence_seen_at.is_none() {
        println!("  g0 never read 3841 across all 600 frames of sustained peak=100 -- classification held stable, not history-dependent under this test.");
    }
}
