// SPDX-License-Identifier: LGPL-3.0-or-later
//! Uses the real chip's own P25 full-rate (RATET 27 family) DECODER as an oracle to empirically
//! discover which of the 144 wire bits are FEC-protected, without assuming any interleave table,
//! byte order, bit direction, or PN-demodulation convention -- all of which the sliding Golay-window
//! scan (`p25_ratet27_sliding_golay_scan.rs`) could not see through, per a from-transcript review.
//!
//! Method: capture one real 144-bit encoded frame R from a steady voiced 200 Hz test tone. Then, for
//! each of the 144 bit positions: re-converge the decoder to R's steady state (send R four more
//! times, discarding output), send R with that single bit flipped, and compare the resulting 160-
//! sample PCM decode to a baseline decode of unmodified R.
//!
//! **Two real dead ends this tool found while being built, both instructive:**
//! 1. Raw time-domain PCM comparison with a voiced 200 Hz test tone doesn't work, even between two
//!    consecutive decodes of the exact same unmodified frame -- the decoder is not glitching, it is
//!    doing the practically necessary, semantically correct thing: continuously tracking pitch phase
//!    across frames for smooth voiced-speech synthesis, so repeated identical-parameter decodes
//!    produce a continuing sinusoid at a *different phase* each time (measured RMS ~8500 between two
//!    genuinely identical-input decodes; both were clearly the same frequency/amplitude tone, just
//!    phase-shifted). That flags every single bit as "changes decode".
//! 2. Switching to digital silence for R (Bruce's suggestion, to sidestep phase entirely) does fix
//!    determinism -- residual RMS drops from ~8500 to ~2 (a small comfort-noise/dither floor, not
//!    exact digital zero) -- but then EVERY one of the 144 bits shows zero effect when flipped, not
//!    just the unprotected 7. The likely reason: for a silence/very-low-energy classified frame, the
//!    decoder probably ignores essentially all of the other encoded parameters and just synthesizes a
//!    fixed low-level comfort-noise pattern based on the classification field alone -- so silence
//!    isn't a stronger test, it's the wrong test vehicle (it makes nearly every bit semantically
//!    irrelevant to the output, independent of FEC).
//!
//! The fix: use a voiced test tone (so every parameter is actually exercised), but compare magnitude
//! spectra in the dB domain rather than raw samples -- phase-invariant (sidesteps dead end #1), and
//! far more sensitive across the full dynamic range than a linear-magnitude spectral distance (which
//! is dominated by the fundamental peak and can mask subtler effects, e.g. from higher-band spectral
//! amplitude or voicing bits that barely perturb a clean low-frequency tone's linear spectrum).
//!
//! A bit flip inside any FEC-protected codeword (Golay(23,12) or Hamming(15,11), per textbook IMBE)
//! is always correctable by that code, so its decoded spectrum should match baseline within the
//! natural phase-evolution noise floor; a flip in one of the unprotected raw bits should shift the
//! spectrum measurably beyond that floor. Expected: exactly 7 "changes decode" positions if this
//! chip's wire format is textbook 4x Golay(23,12) + 3x Hamming(15,11) + 7 raw bits (144 = 92+45+7),
//! regardless of what order/interleave those blocks are actually transmitted in -- a single bit's
//! membership in "some correctable codeword" survives any linear transformation (byte reorder, bit
//! reversal, or XOR against a PN sequence) applied identically to both frames being compared.
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const TEST_FREQ_HZ: f64 = 200.0;
const SETTLING_FRAMES: usize = 80;
const TOTAL_BITS: usize = 144;
const TOTAL_BYTES: usize = TOTAL_BITS / 8;
const PRIME_REPEATS: usize = 4;
const BASELINE_REPEATS: usize = 5;

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
// Bit position k (0-indexed, MSB-first) within the 144-bit channel field lives at absolute
// buffer offset BITS_OFFSET + k/8, bit 7-(k%8) -- payload starts at buf[4] (after
// START_BYTE/LENGTH/TYPE), and within the payload, bits start at payload[2] (after the channel
// packet's own field-ID and bit-count bytes).
const BITS_OFFSET: usize = 6;
fn flip_bit_in_packet(raw_packet: &[u8], k: usize) -> Vec<u8> {
    let mut pkt = raw_packet.to_vec();
    pkt[BITS_OFFSET + k / 8] ^= 1 << (7 - (k % 8));
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
// A voiced tone plus a fixed, repeatable pseudo-random noise floor -- meant to give the encoder
// real broadband/unvoiced spectral content (which a clean sinusoid alone has essentially none of)
// so unvoiced-band parameters get real, non-degenerate values, while staying perfectly repeatable
// frame-to-frame (same 160-sample buffer resent every settling frame, so the encoder still
// converges to one stable reference frame R) -- a real xorshift PRNG, not the system RNG, so this
// tool has no external random-number-generator dependency and the same sequence reproduces exactly
// on every run.
fn test_tone_plus_noise(freq: f64, noise_amplitude: i16) -> Vec<i16> {
    let tone = test_tone(freq);
    let mut state: u32 = 0x12345678;
    let mut next_i16 = || -> i16 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        ((state as i32 % (2 * noise_amplitude as i32 + 1)) - noise_amplitude as i32) as i16
    };
    tone.iter().map(|&s| s.saturating_add(next_i16())).collect()
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    // "tone" (default) or "tone_noise" -- the latter adds a fixed, repeatable noise floor to give
    // the encoder real broadband/unvoiced spectral content a pure tone alone has essentially none
    // of, in case the tone-only run misses unprotected bits controlling unvoiced-band parameters.
    let signal_kind = std::env::args().nth(2).unwrap_or_else(|| "tone".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
    println!("RATEP(P25 FEC) config ack: type={ptype:#04x} payload={payload:02x?}");

    // Capture one real steady-state encoded frame R -- silence was tried first and rejected (see
    // module doc comment): it makes almost every bit semantically irrelevant to the decoder's
    // output, not just the unprotected ones.
    let samples = match signal_kind.as_str() {
        "tone" => test_tone(TEST_FREQ_HZ),
        "tone_noise" => test_tone_plus_noise(TEST_FREQ_HZ, 1500),
        other => panic!("unknown signal kind {other:?}, expected \"tone\" or \"tone_noise\""),
    };
    println!("using test signal: {signal_kind}");
    let mut r: Vec<u8> = Vec::new();
    for i in 0..SETTLING_FRAMES {
        sock.send(&build_speech(&samples)).expect("send speech");
        let n = sock.recv(&mut buf).expect("recv channel");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let num_bits = payload[1] as usize;
        assert_eq!(num_bits, TOTAL_BITS, "expected 144 bits for RATEP P25 FEC");
        if i == SETTLING_FRAMES - 1 {
            r = buf[..n].to_vec();
        }
    }
    println!("captured reference frame R (raw packet, {} bytes): {r:02x?}", r.len());
    assert!(r.len() >= BITS_OFFSET + TOTAL_BYTES, "captured packet too short to hold 144 bits");

    let send_channel_get_pcm = |sock: &UdpSocket, buf: &mut [u8; 512], raw_packet: &[u8]| -> Vec<i16> {
        sock.send(raw_packet).expect("send channel");
        let n = sock.recv(buf).expect("recv speech");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decode) response");
        payload[2..].chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]])).collect()
    };

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(FRAME_SAMPLES);
    // dB-magnitude spectrum: far more sensitive across the full dynamic range than linear
    // magnitude, which is dominated by the fundamental peak and can mask subtler effects.
    const DB_FLOOR: f64 = 1.0; // avoids log(0); dwarfed by any real signal component
    let db_spectrum = |samples: &[i16]| -> Vec<f64> {
        let mut buf: Vec<Complex64> = samples.iter().map(|&s| Complex64::new(s as f64, 0.0)).collect();
        fft.process(&mut buf);
        buf[..FRAME_SAMPLES / 2 + 1]
            .iter()
            .map(|c| 20.0 * (c.norm().max(DB_FLOOR)).log10())
            .collect()
    };
    let spectral_distance = |a: &[f64], b: &[f64]| -> f64 {
        let sum_sq: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum();
        (sum_sq / a.len() as f64).sqrt()
    };

    // Converge decoder to R's steady state, then establish baseline dB spectrum. Magnitude
    // spectrum is phase-invariant, sidestepping the continuous-phase-tracking dead end documented
    // above; the dB domain (vs. linear magnitude) gives much better sensitivity across the full
    // dynamic range instead of being dominated by the fundamental peak alone.
    for _ in 0..BASELINE_REPEATS - 1 {
        send_channel_get_pcm(&sock, &mut buf, &r);
    }
    let baseline_pcm = send_channel_get_pcm(&sock, &mut buf, &r);
    let baseline_spectrum = db_spectrum(&baseline_pcm);

    // Calibrate the natural noise floor: repeated decodes of the SAME unmodified frame R, re-
    // priming between each exactly like the real per-bit test loop will.
    let mut floor_distances = Vec::new();
    for i in 0..8 {
        for _ in 0..PRIME_REPEATS {
            send_channel_get_pcm(&sock, &mut buf, &r);
        }
        let pcm = send_channel_get_pcm(&sock, &mut buf, &r);
        let distance = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm));
        floor_distances.push(distance);
        println!("noise-floor calibration {}/8: spectral distance={distance:.2} dB", i + 1);
    }
    let noise_floor = floor_distances.iter().cloned().fold(0.0_f64, f64::max);
    let threshold = (noise_floor * 5.0).max(10.0);
    println!("\nnoise floor (max of 8 repeats): {noise_floor:.2} dB; using changed-decode threshold = {threshold:.2} dB");

    println!("\n--- single-bit-flip sensitivity scan across all {TOTAL_BITS} wire bit positions ---");
    let mut changed_positions = Vec::new();
    let mut all_distances = Vec::new();
    for k in 0..TOTAL_BITS {
        // Re-converge to steady state.
        for _ in 0..PRIME_REPEATS {
            send_channel_get_pcm(&sock, &mut buf, &r);
        }
        let flipped = flip_bit_in_packet(&r, k);
        let pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
        let distance = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm));
        all_distances.push(distance);
        let changed = distance > threshold;
        if changed {
            changed_positions.push(k);
        }
        print!("{}", if changed { 'X' } else { '.' });
        if (k + 1) % 24 == 0 {
            println!();
        }
    }
    println!();
    println!("all spectral distances (dB): {all_distances:.2?}");

    println!(
        "\n{} of {TOTAL_BITS} bit positions changed decoded PCM when flipped (expect exactly 7 if textbook 4xGolay(23,12)+3xHamming(15,11)+7raw):",
        changed_positions.len()
    );
    println!("changed positions: {changed_positions:?}");
    // Re-converge once more at the end, leaving the decoder in a clean state.
    for _ in 0..PRIME_REPEATS {
        send_channel_get_pcm(&sock, &mut buf, &r);
    }
}
