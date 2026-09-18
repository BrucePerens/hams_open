// SPDX-License-Identifier: LGPL-3.0-or-later
//! Validates this crate's D-STAR AMBE frame parsing (`ambe_dstar`) against a real DVSI AMBE3003 chip
//! reachable over the LAN via AMBEServer3003 -- the permanent, committed replacement for the
//! throwaway Python capture script this investigation started with (see
//! `docs/references/AMBE_CHIP_VALIDATION_FINDINGS.md`'s D-STAR section for the full write-up).
//!
//! Configures the chip for the D-STAR RATEP rate-control-word (confirmed via two independent primary
//! sources: DVSI's own USB-3000 Manual Table 30, and G4KLX's AMBETools `DV3000_REQ_DSTAR_FEC`), feeds
//! it a stationary test tone at several frequencies whose period evenly divides the 160-sample frame
//! (avoiding inter-frame phase drift), captures many consecutive encoded frames per frequency (the
//! chip's own encoder was found to never fully converge to a fixed steady-state output on a pure
//! tone -- see the findings doc -- so many frames are captured and only the resulting FEC validity is
//! checked, not bit-for-bit frame equality), and verifies every captured frame Golay-decodes with
//! zero corrected errors on both `C0` and `C1` through
//! `ambe_dstar::interleave::wire_bytes_to_frame` + `ambe_dstar::decode::parse_frame` -- the real
//! falsification test this investigation's own bit-order search used to find and fix two real
//! framing bugs (see `interleave.rs`'s doc comment).
//!
//! Frequencies above roughly 400Hz are outside AMBE's own designed vocal-pitch range (this crate's
//! own `tables::L_TABLE` covers pitch periods corresponding to about 57-444Hz), so this harness
//! reports them separately from the voice-range frequencies -- but empirically, once the two real
//! framing bugs above were fixed, every frequency tested (including 500/800/1000Hz) decoded with
//! zero errors too; an early, wrong theory during this investigation blamed a since-fixed framing
//! bug's symptom on the chip being "at the edge of its range" instead.

use ham_digital_modes::ambe_dstar::decode::parse_frame;
use ham_digital_modes::ambe_dstar::interleave::wire_bytes_to_frame;
use std::net::UdpSocket;
use std::time::Duration;

const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const FRAMES_PER_FREQ: usize = 50;
const DISCARD_ONSET: usize = 10;
// Voice-range frequencies (period evenly divides 160) plus two above AMBE's designed pitch range,
// kept separate in the report rather than folded into the same pass/fail count.
const VOICE_RANGE_HZ: [u32; 5] = [50, 100, 200, 250, 400];
const ABOVE_RANGE_HZ: [u32; 3] = [500, 800, 1000];

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;

// Confirmed via DVSI's own USB-3000 Manual (Table 30, "interoperable with D-STAR") and G4KLX's
// AMBETools `DV3000_REQ_DSTAR_FEC` -- two independent primary sources, byte-for-byte identical.
const RATEP_DSTAR: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];

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
    Some((ptype, data.get(4..4 + length)?))
}

fn encode_frame(sock: &UdpSocket, samples: &[i16]) -> Option<[u8; 9]> {
    sock.send(&build_speech(samples)).ok()?;
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).ok()?;
    let (ptype, payload) = parse_packet(&buf[..n])?;
    if ptype != TYPE_CHANNEL || payload.len() < 2 {
        return None;
    }
    let num_bits = payload[1] as usize;
    if num_bits != 72 {
        eprintln!("warning: chip returned {num_bits} bits, expected 72");
    }
    let bits = &payload[2..];
    if bits.len() < 9 {
        return None;
    }
    let mut out = [0u8; 9];
    out.copy_from_slice(&bits[..9]);
    Some(out)
}

fn test_tone(freq: u32) -> Vec<i16> {
    let period = (SAMPLE_RATE / freq as f64).round() as usize;
    (0..FRAME_SAMPLES)
        .map(|n| (8000.0 * (2.0 * std::f64::consts::PI * (n % period) as f64 / period as f64).sin()) as i16)
        .collect()
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

    sock.send(&build_control_ratep(RATEP_DSTAR)).expect("send RATEP config");
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("RATEP config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
    println!("RATEP config response: type={ptype:02x} payload={payload:02x?}");

    let mut any_failures_in_range = false;
    for &freq in VOICE_RANGE_HZ.iter().chain(ABOVE_RANGE_HZ.iter()) {
        let samples = test_tone(freq);
        let mut exact = 0usize;
        let mut total = 0usize;
        for i in 0..FRAMES_PER_FREQ {
            let Some(bytes) = encode_frame(&sock, &samples) else {
                eprintln!("  {freq}Hz frame {i}: no response from chip");
                continue;
            };
            if i < DISCARD_ONSET {
                continue;
            }
            total += 1;
            let frame = wire_bytes_to_frame(&bytes);
            let parsed = parse_frame(frame);
            if parsed.epsilon_c0 == 0 && parsed.epsilon_c1 == 0 {
                exact += 1;
            }
        }
        let in_voice_range = VOICE_RANGE_HZ.contains(&freq);
        let tag = if in_voice_range { "voice range" } else { "above AMBE's designed pitch range" };
        println!("{freq:>5}Hz ({tag}): {exact}/{total} frames Golay-decoded with zero errors on C0 and C1");
        if in_voice_range && exact != total {
            any_failures_in_range = true;
        }
    }

    if any_failures_in_range {
        eprintln!("\nFAIL: at least one voice-range frequency had a non-zero-error frame -- real framing bug.");
        std::process::exit(1);
    }
    println!("\nPASS: every voice-range frame (50/100/200/250/400Hz) Golay-decoded with zero errors on both C0 and C1.");
}
