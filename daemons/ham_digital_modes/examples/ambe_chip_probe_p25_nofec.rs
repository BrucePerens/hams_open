// SPDX-License-Identifier: LGPL-3.0-or-later
//! Probes the real DVSI AMBE3003 chip's P25 **NOFEC** mode (RATEP word confirmed from a real
//! working reference, G4KLX AMBETools' `DV3000SerialController.cpp`, `DV3000_REQ_P25_NOFEC`) --
//! unlike the FEC-protected `CHANNEL` output every other P25 harness in this crate uses, NOFEC mode
//! returns the raw, unprotected, unwhitened 88-bit voice frame directly (confirmed live: exactly 88
//! bits every time, matching this crate's own `VOICE_BITS` exactly). This removes every FEC/
//! whitening/interleave ambiguity from the P25 wire-format mystery at a stroke: there is no Golay,
//! no Hamming, no PRN whitening, and (per DVSI's own manual and AMBETools' own RCW naming) no reason
//! to expect any interleave either, since interleaving exists specifically to protect FEC-coded bits
//! from burst errors -- a mode with no FEC at all has nothing left to interleave. Whatever bit order
//! this mode uses should be the *chip's own real internal field order*, not obscured by any of the
//! confounds the FEC-mode wire-format search (`ambe_chip_validate_p25_wireformat.rs`) had to work
//! around.
//!
//! # What's confirmed so far (see `docs/references/AMBE_CHIP_VALIDATION_FINDINGS.md` for detail)
//!
//! - The chip's NOFEC output **converges to a perfectly stable, exactly-repeating value** for a
//!   stationary tone whose period evenly divides 160 samples (unlike the D-STAR chip's own low-
//!   frequency non-convergence) -- confirmed across 200/400/500/1000Hz, 40 frames each, zero
//!   deviation after settling.
//! - **Bit-diffing the raw 88-bit frames between different test frequencies** (the same technique
//!   that found P25 FEC mode's own "stride-12" clue) shows only 3-5 of the 88 bits actually change
//!   between any pair of the frequencies tested, and those bits are *not* clustered in the first 12
//!   bits (where this crate's own `u0`-first, MSB-first, contiguous convention would put the pitch
//!   parameter) -- they're spread across bit positions in what would be `u1`, `u2`, `u4`, `u5`, `u6`,
//!   and `u7` under that convention, never `u0` or `u3`. This is a real, structural clue: several
//!   fields shifting together, rather than one field varying smoothly, is consistent with a harmonic-
//!   count-dependent bit-allocation boundary effect (changing pitch shifts `L_hat`, which shifts how
//!   many bits several *other* parameters get) -- but it does not yet pin down a specific "this is
//!   the pitch field" answer, and is worth further investigation rather than a settled conclusion.
//!
//! Run against the chip: `cargo run --release --example ambe_chip_probe_p25_nofec -- 192.168.10.189:2460`.
//!
//! # Safety
//! Only ever sends ordinary DVSI CONTROL/SPEECH UDP packets, exactly like every other committed
//! `ambe_chip_*` harness -- never touches the serial/USB layer directly.

use std::net::UdpSocket;
use std::time::Duration;

const RATEP_P25_NOFEC: [u16; 6] = [0x0558, 0x086B, 0x0000, 0x0000, 0x0000, 0x0158];
const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const SETTLING_FRAMES: usize = 30;
const CAPTURED_FRAMES: usize = 10;

// Voice-range frequencies whose period evenly divides 160 samples (avoids inter-frame phase drift
// between "settled" frames -- the same confound found and fixed during the D-STAR investigation).
const TEST_FREQS_HZ: [f64; 4] = [200.0, 400.0, 500.0, 1000.0];

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

fn test_tone(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (8000.0 * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
        .collect()
}

fn bit_diff_positions(a: &[u8], b: &[u8], num_bits: usize) -> Vec<usize> {
    let mut positions = Vec::new();
    for i in 0..num_bits {
        let byte_i = i / 8;
        let bit_i = 7 - (i % 8);
        let a_bit = (a[byte_i] >> bit_i) & 1;
        let b_bit = (b[byte_i] >> bit_i) & 1;
        if a_bit != b_bit {
            positions.push(i);
        }
    }
    positions
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    sock.send(&build_control_ratep(RATEP_P25_NOFEC)).expect("send RATEP config");
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("RATEP config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
    println!("RATEP(P25 NOFEC) config ack: type={ptype:#04x} payload={payload:02x?}");

    let mut settled: Vec<(f64, Vec<u8>, usize)> = Vec::new();
    for &freq in &TEST_FREQS_HZ {
        let samples = test_tone(freq);
        let mut num_bits = 0usize;
        let mut last_frame = Vec::new();
        let mut stable_across_capture = true;
        for i in 0..(SETTLING_FRAMES + CAPTURED_FRAMES) {
            sock.send(&build_speech(&samples)).expect("send speech");
            let n = sock.recv(&mut buf).expect("recv channel");
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
            num_bits = payload[1] as usize;
            let nbytes = num_bits.div_ceil(8);
            let frame = payload[2..2 + nbytes].to_vec();
            if i > SETTLING_FRAMES && frame != last_frame {
                stable_across_capture = false;
            }
            last_frame = frame;
        }
        println!(
            "{freq:>6}Hz: num_bits={num_bits} frame={} stable={stable_across_capture}",
            last_frame.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        settled.push((freq, last_frame, num_bits));
    }

    println!("\n--- Bit-diff between frequency pairs (localizes where pitch information lives) ---");
    for i in 0..settled.len() {
        for j in (i + 1)..settled.len() {
            let (fa, ba, na) = &settled[i];
            let (fb, bb, nb) = &settled[j];
            assert_eq!(na, nb, "frame sizes must match to diff");
            let diff = bit_diff_positions(ba, bb, *na);
            println!("{fa:>6}Hz vs {fb:>6}Hz: {} bits differ at positions {diff:?}", diff.len());
        }
    }
}
