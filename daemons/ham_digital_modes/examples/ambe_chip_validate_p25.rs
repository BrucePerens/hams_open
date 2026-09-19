// SPDX-License-Identifier: LGPL-3.0-or-later
//! Validates this crate's from-spec AMBE (P25 CAI, TIA-102.BABA-derived) encoder against a real
//! DVSI AMBE3003 chip reachable over the LAN via AMBEServer3003 (see
//! `docs/AMBE_CHIP_VALIDATION_FINDINGS.md` for the full writeup and how this harness is used).
//!
//! Configures the chip for RATET index 27 (7200 total / 4400 speech / 2800 FEC bps, per DVSI's own
//! published rate table -- exactly this crate's own 144-bit frame: `VOICE_BITS=88` + `FEC_BITS=56`),
//! feeds it a stationary synthetic tone over a real UDP round trip, and runs the identical signal
//! through this crate's own encoder using the tone's exact, analytically-known pitch (bypassing this
//! crate's own pitch tracker entirely -- deliberately: a mismatch in the pitch tracker's own
//! candidate-selection logic is a different, separable question from whether the quantization/DCT/FEC
//! tables downstream of a *correct* pitch estimate match the chip bit-for-bit, which is what this
//! harness isolates first).
//!
//! Since the chip's own real-time pitch tracker and this crate's own reconstructed-history feedback
//! both need a few frames to settle from a cold start, and the real chip's own internal algorithmic
//! lookahead is not assumed to align frame-for-frame with this harness's own indexing, this harness
//! searches a small window of frame-index offsets between the two streams and reports the offset
//! with the most bit agreement, rather than assuming naive index-for-index alignment.

use ham_digital_modes::ambe::float::tia_102_baba::{encode_frame, pitch_refinement::RefinementFrame, FrameState};
use std::net::UdpSocket;

const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160; // 20ms at 8kHz
const TONE_HZ: f64 = 200.0;
const NUM_FRAMES: usize = 40;
const COMPARE_FROM: usize = 8; // skip early frames while both sides settle

fn build_control_ratet(index: u8) -> Vec<u8> {
    let payload = vec![0x09_u8, index];
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(0x00);
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
    pkt.push(0x02); // TYPE_SPEECH
    pkt.extend_from_slice(&payload);
    pkt
}

/// Parses a DVSI packet, returning (type, payload).
fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    Some((ptype, &data[4..4 + length]))
}

/// Packs c[0..8] (widths 23,23,23,23,15,15,15,7 = 144 bits) MSB-first into 18 bytes -- the natural,
/// standard bit-packing convention (and the one this harness empirically confirms against the real
/// chip's own output, see the findings doc).
fn pack_frame_msb_first(c: [u32; 8]) -> [u8; 18] {
    let widths = [23u32, 23, 23, 23, 15, 15, 15, 7];
    let mut bits: Vec<bool> = Vec::with_capacity(144);
    for (&val, &width) in c.iter().zip(widths.iter()) {
        for b in (0..width).rev() {
            bits.push((val >> b) & 1 == 1);
        }
    }
    let mut out = [0u8; 18];
    for (i, chunk) in bits.chunks(8).enumerate() {
        let mut byte = 0u8;
        for (j, &bit) in chunk.iter().enumerate() {
            if bit {
                byte |= 1 << (7 - j);
            }
        }
        out[i] = byte;
    }
    out
}

fn hamming_distance(a: &[u8], b: &[u8]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

// `i` is used both to index `our_frames` and, via `i as i32 + offset`, to compute an independent
// index into `chip_frames` -- the offset arithmetic genuinely needs it as an integer, so clippy's
// suggested `enumerate()` rewrite doesn't fit.
#[allow(clippy::needless_range_loop)]
fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).map(|s| s.as_str()).unwrap_or("192.168.10.189");
    let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2460);

    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.connect((host, port))?;
    sock.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;

    // Configure RATET index 27: 7200 total / 4400 speech / 2800 FEC bps.
    sock.send(&build_control_ratet(27))?;
    let mut buf = [0u8; 4096];
    let n = sock.recv(&mut buf)?;
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid RATET ack");
    println!(
        "RATET(27) config ack: type={:#04x} payload={:02x?}",
        ptype, payload
    );

    // Generate a long stationary tone and feed it to the chip 160 samples at a time, collecting
    // every real encoded frame it returns.
    let total_samples = NUM_FRAMES * FRAME_SAMPLES + 400; // pad for our own encoder's own window margin
    let raw: Vec<f64> = (0..total_samples)
        .map(|n| 8000.0 * (2.0 * std::f64::consts::PI * TONE_HZ * n as f64 / SAMPLE_RATE).sin())
        .collect();

    let mut chip_frames: Vec<[u8; 18]> = Vec::with_capacity(NUM_FRAMES);
    for frame_idx in 0..NUM_FRAMES {
        let start = frame_idx * FRAME_SAMPLES;
        let samples: Vec<i16> = raw[start..start + FRAME_SAMPLES]
            .iter()
            .map(|&s| s.round() as i16)
            .collect();
        sock.send(&build_speech(&samples))?;
        let n = sock.recv(&mut buf)?;
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid channel response");
        assert_eq!(ptype, 0x01, "expected a CHANNEL (encoded AMBE) response");
        let num_bits = payload[1];
        assert_eq!(num_bits, 144, "expected a 144-bit frame at RATET(27)");
        let mut frame = [0u8; 18];
        frame.copy_from_slice(&payload[2..20]);
        chip_frames.push(frame);
    }
    println!("Collected {} real chip-encoded frames.", chip_frames.len());

    // Run the identical tone through our own from-spec encoder, using the tone's exact,
    // analytically-known pitch (bypassing our own pitch tracker for this first pass -- see module
    // doc comment above).
    let omega0_hat = 2.0 * std::f64::consts::PI * TONE_HZ / SAMPLE_RATE;
    let mut state = FrameState::initial();
    let mut our_frames: Vec<[u8; 18]> = Vec::with_capacity(NUM_FRAMES);
    for frame_idx in 0..NUM_FRAMES {
        let center = frame_idx * FRAME_SAMPLES + FRAME_SAMPLES / 2 + 110; // real margin for RefinementFrame::new's own +-110 need
        let frame = RefinementFrame::new(&raw, center);
        let (c, next_state) = encode_frame(&frame, omega0_hat, 0.001, &state, false)
            .expect("a clean stationary tone should always encode");
        our_frames.push(pack_frame_msb_first(c));
        state = next_state;
    }
    println!("Produced {} of our own encoder's frames.", our_frames.len());

    // Search a small offset window for the best-agreeing alignment between the two streams, rather
    // than assuming naive index-for-index alignment (real chips commonly have their own algorithmic
    // lookahead/delay).
    println!(
        "\nOffset search (comparing frames {}..{}):",
        COMPARE_FROM,
        NUM_FRAMES - 4
    );
    let mut best_offset = 0i32;
    let mut best_bits_matched = 0u32;
    for offset in -4i32..=4 {
        let mut total_bits = 0u32;
        let mut matched_bits = 0u32;
        for i in COMPARE_FROM..NUM_FRAMES - 4 {
            let j = i as i32 + offset;
            if j < 0 || j as usize >= chip_frames.len() {
                continue;
            }
            let dist = hamming_distance(&our_frames[i], &chip_frames[j as usize]);
            total_bits += 144;
            matched_bits += 144 - dist;
        }
        let pct = 100.0 * matched_bits as f64 / total_bits.max(1) as f64;
        println!(
            "  offset {:+}: {}/{} bits match ({:.2}%)",
            offset, matched_bits, total_bits, pct
        );
        if matched_bits > best_bits_matched {
            best_bits_matched = matched_bits;
            best_offset = offset;
        }
    }

    println!(
        "\nBest offset: {:+} ({} bits matched)",
        best_offset, best_bits_matched
    );
    println!("\nPer-frame hex dump at best offset:");
    for i in COMPARE_FROM..NUM_FRAMES - 4 {
        let j = i as i32 + best_offset;
        if j < 0 || j as usize >= chip_frames.len() {
            continue;
        }
        let ours = &our_frames[i];
        let chip = &chip_frames[j as usize];
        let dist = hamming_distance(ours, chip);
        println!(
            "  frame {:2}: ours={} chip={} hamming_dist={}",
            i,
            hex(ours),
            hex(chip),
            dist
        );
    }

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
