// SPDX-License-Identifier: LGPL-3.0-or-later
//! Diagnoses the residual `~0.63` chip-vs-float envelope correlation gap RATET(27) synthesis has
//! against the real DVSI chip (`night_shift_todo/medium/ambe-fixed-point-port-f0c555a5.md` in
//! hams_com has the full history). One concrete hypothesis under test, from a direct question: does
//! the chip's own synthesis extend harmonic content beyond `L_hat` (spectral/harmonic bandwidth
//! extension), which this crate's from-spec synthesis never does?
//!
//! Selects frames this crate's own decoder classifies as fully voiced (every harmonic voiced -- the
//! cleanest case, since Eq. 133's own steady-state branch produces an exact line spectrum at
//! `k*omega0` for `k=1..=L_hat` with nothing in between). FFTs both the chip's real PCM and this
//! crate's float-synthesized PCM for those frames, then reports, per frame:
//! - the magnitude ratio (chip/float) at each harmonic line `k=1..=L_hat`,
//! - the energy chip carries *above* `L_hat*omega0` (the frequency this crate's own synthesis has
//!   deliberately zero energy above, since nothing is voiced past `L_hat`) as a fraction of total
//!   energy, compared to the same fraction in the float PCM,
//! - the noise floor *between* harmonic lines (energy at non-harmonic bins) for both.
//!
//! This discriminates the real candidate explanations directly rather than guessing: extra energy
//! above `L_hat*omega0` specific to the chip -> harmonic extension. A broadband floor between lines on
//! the chip but not float -> comfort noise/dither fill, not extension. A consistent magnitude tilt
//! across `k` -> a spectral-enhancement/smoothing difference, not extension. A ratio that varies with
//! `L_hat` or gain (`b2`) -> a gain-reconstruction difference.
//!
//! **Result (live chip, 10 predominantly-voiced frames)**: energy above `L_hat*omega0` is `0.87%` of
//! total for the chip vs `0.39%` for float -- both tiny, so harmonic extension is not the main gap.
//! Chip/float line-magnitude ratio is ~3-4x for harmonics 7-12 and ~1-2x above, unexplained. In every
//! selected frame harmonics 1-6 decode unvoiced (see `ratet27_probe_voicing_bit_order.rs`).
//!
//! Usage: `cargo run --release --example ratet27_diagnose_harmonic_extension -- <host:port>`

use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 200;

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
fn build_channel(payload: &[u8]) -> Vec<u8> {
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CHANNEL);
    pkt.extend_from_slice(payload);
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
fn parse_speech_payload(payload: &[u8]) -> Vec<i16> {
    let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    payload[2..2 + count * 2]
        .chunks_exact(2)
        .map(|b| i16::from_be_bytes([b[0], b[1]]))
        .collect()
}
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 1024], pkt: &[u8]) -> usize {
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
}
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}
fn wire_bytes_to_c(bytes: &[u8; FRAME_BYTES]) -> [u32; 8] {
    let mut wire_frame_bits = [false; 144];
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for b in 0..8 {
            wire_frame_bits[byte_idx * 8 + b] = (byte >> (7 - b)) & 1 == 1;
        }
    }
    let raw = |block: Block| -> u32 {
        let members = block_wire_members(block);
        let mut received: u32 = 0;
        for (offset, &wire) in members.iter().enumerate() {
            if wire_frame_bits[wire] {
                received |= 1 << (members.len() - 1 - offset);
            }
        }
        received
    };
    [
        raw(Block::Golay { index: 0 }),
        raw(Block::Golay { index: 1 }),
        raw(Block::Golay { index: 2 }),
        raw(Block::Golay { index: 3 }),
        raw(Block::Hamming { index: 0 }),
        raw(Block::Hamming { index: 1 }),
        raw(Block::Hamming { index: 2 }),
        raw(Block::Raw),
    ]
}

/// A real (not naive-DFT) FFT via rustfft, zero-padded to `fft_len` for finer bin resolution than
/// the raw 160-sample frame alone would give (160 samples at 8kHz gives only 50Hz bins; zero-padding
/// to 1024 gives ~7.8Hz bins, enough to cleanly separate harmonic lines from the AMBE pitch range).
fn fft_magnitude(samples: &[f64], fft_len: usize) -> Vec<f64> {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(fft_len);
    let mut buf: Vec<Complex64> = samples.iter().map(|&s| Complex64::new(s, 0.0)).collect();
    buf.resize(fft_len, Complex64::new(0.0, 0.0));
    fft.process(&mut buf);
    buf.iter().map(|c| c.norm()).collect()
}

/// Nearest FFT bin index for a frequency in Hz.
fn bin_for_hz(hz: f64, fft_len: usize, sample_rate: f64) -> usize {
    ((hz * fft_len as f64 / sample_rate).round() as usize).min(fft_len / 2)
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let pcm = read_wav_mono_i16("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    let n_frames = (pcm.len() / FRAME_SAMPLES).min(N_FRAMES);

    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        channel_payloads.push(payload.to_vec());
    }

    // Decode-only pass (no synthesis needed for classification) to find fully-voiced frames and
    // their own l_hat/omega0_tilde, using a separate decoder instance driven only through
    // decode_parameters + advance_history (mirrors decode_frame's own history update exactly).
    let mut params_decoder = DecoderState::new();
    // (frame index, l_hat, omega0_tilde, per-harmonic voiced, fraction voiced)
    let mut fully_voiced_frames: Vec<(usize, u32, f64, Vec<bool>)> = Vec::new();
    let mut voiced_fraction_histogram: Vec<f64> = Vec::new();
    for (i, channel_payload) in channel_payloads.iter().enumerate() {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);
        if let Some(FrameOutcome::Decoded(params)) = params_decoder.decode_parameters(c) {
            let voiced_count = params.voiced.iter().filter(|&&v| v).count();
            let frac = voiced_count as f64 / params.voiced.len().max(1) as f64;
            voiced_fraction_histogram.push(frac);
            // Predominantly voiced (>=70% of harmonics), not requiring every single one -- real
            // speech rarely has literally every harmonic voiced even in strongly-voiced frames.
            if frac >= 0.7 && params.l_hat >= 8 {
                fully_voiced_frames.push((i, params.l_hat, params.omega0_tilde, params.voiced.clone()));
            }
            params_decoder.advance_history(&params);
        }
    }
    println!(
        "{} of {n_frames} frames are >=70% voiced with l_hat>=8 (the clean line-spectrum case).",
        fully_voiced_frames.len()
    );
    for (idx, l_hat, omega0_tilde, voiced) in &fully_voiced_frames {
        let fundamental_hz = omega0_tilde * 8000.0 / (2.0 * std::f64::consts::PI);
        let pattern: String = voiced.iter().map(|&v| if v { '1' } else { '0' }).collect();
        println!("  frame {idx}: l_hat={l_hat}, f0={fundamental_hz:.1}Hz, voiced=[{pattern}]");
    }
    println!(
        "voiced-fraction histogram: min={:.2} max={:.2} mean={:.2}",
        voiced_fraction_histogram.iter().cloned().fold(f64::INFINITY, f64::min),
        voiced_fraction_histogram.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        voiced_fraction_histogram.iter().sum::<f64>() / voiced_fraction_histogram.len().max(1) as f64
    );

    // Full decode pass (chip PCM + float PCM), same two-pass-not-interleaved shape as the other
    // chip-comparison harnesses.
    let mut float_decoder = DecoderState::new();
    let mut chip_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut float_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    for channel_payload in &channel_payloads {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);

        let n = send_recv_retrying(&sock, &mut buf, &build_channel(channel_payload));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decoded PCM) response");
        let chip_frame_pcm = parse_speech_payload(payload);
        chip_pcm.extend(chip_frame_pcm.iter().map(|&s| s as f64));

        match float_decoder.decode_frame(c) {
            Some(frame_pcm) => float_pcm.extend(frame_pcm.iter().copied()),
            None => float_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES)),
        }
    }

    const FFT_LEN: usize = 1024;
    const SAMPLE_RATE: f64 = 8000.0;
    let nyquist_bin = FFT_LEN / 2;

    // Aggregate, across every fully-voiced frame, the per-harmonic magnitude ratio (chip/float) and
    // the above-L_hat*omega0 / between-lines energy fractions for both streams.
    let mut ratio_by_relative_harmonic: Vec<Vec<f64>> = vec![Vec::new(); 20]; // k=1..=20 (most frames' l_hat)
    let mut chip_above_frac = Vec::new();
    let mut float_above_frac = Vec::new();
    let mut chip_between_frac = Vec::new();
    let mut float_between_frac = Vec::new();

    for (frame_idx, l_hat, omega0_tilde, voiced) in &fully_voiced_frames {
        let (frame_idx, l_hat, omega0_tilde) = (*frame_idx, *l_hat, *omega0_tilde);
        let chip_frame = &chip_pcm[frame_idx * FRAME_SAMPLES..(frame_idx + 1) * FRAME_SAMPLES];
        let float_frame = &float_pcm[frame_idx * FRAME_SAMPLES..(frame_idx + 1) * FRAME_SAMPLES];
        let chip_mag = fft_magnitude(chip_frame, FFT_LEN);
        let float_mag = fft_magnitude(float_frame, FFT_LEN);

        let fundamental_hz = omega0_tilde * SAMPLE_RATE / (2.0 * std::f64::consts::PI);
        let cutoff_hz = (l_hat as f64 + 0.5) * fundamental_hz;
        let cutoff_bin = bin_for_hz(cutoff_hz, FFT_LEN, SAMPLE_RATE).min(nyquist_bin);

        let total_chip: f64 = chip_mag[..nyquist_bin].iter().map(|&m| m * m).sum();
        let total_float: f64 = float_mag[..nyquist_bin].iter().map(|&m| m * m).sum();
        if total_chip <= 1e-9 || total_float <= 1e-9 {
            continue;
        }
        let above_chip: f64 = chip_mag[cutoff_bin..nyquist_bin].iter().map(|&m| m * m).sum();
        let above_float: f64 = float_mag[cutoff_bin..nyquist_bin].iter().map(|&m| m * m).sum();
        chip_above_frac.push(above_chip / total_chip);
        float_above_frac.push(above_float / total_float);

        // Per-harmonic line magnitude ratio, and between-lines noise floor (midpoints between
        // consecutive harmonics, up to l_hat).
        let mut between_chip_energy = 0.0;
        let mut between_float_energy = 0.0;
        for k in 1..=l_hat.min(20) {
            if !voiced.get((k - 1) as usize).copied().unwrap_or(false) {
                continue; // Only a genuinely voiced harmonic produces a real line here.
            }
            let line_hz = k as f64 * fundamental_hz;
            let line_bin = bin_for_hz(line_hz, FFT_LEN, SAMPLE_RATE);
            if line_bin == 0 || line_bin >= nyquist_bin {
                continue;
            }
            // A small window around the line bin (+-1) to tolerate quantization of bin_for_hz.
            let chip_line: f64 = ((line_bin.saturating_sub(1))..=(line_bin + 1).min(nyquist_bin - 1))
                .map(|b| chip_mag[b])
                .fold(0.0, f64::max);
            let float_line: f64 = ((line_bin.saturating_sub(1))..=(line_bin + 1).min(nyquist_bin - 1))
                .map(|b| float_mag[b])
                .fold(0.0, f64::max);
            if float_line > 1e-6 {
                ratio_by_relative_harmonic[(k - 1) as usize].push(chip_line / float_line);
            }

            if k < l_hat {
                let mid_hz = (k as f64 + 0.5) * fundamental_hz;
                let mid_bin = bin_for_hz(mid_hz, FFT_LEN, SAMPLE_RATE);
                if mid_bin > 0 && mid_bin < nyquist_bin {
                    between_chip_energy += chip_mag[mid_bin] * chip_mag[mid_bin];
                    between_float_energy += float_mag[mid_bin] * float_mag[mid_bin];
                }
            }
        }
        chip_between_frac.push(between_chip_energy / total_chip);
        float_between_frac.push(between_float_energy / total_float);
    }

    let avg = |v: &[f64]| if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 };

    println!("\n--- Energy above L_hat*omega0 (as a fraction of total frame energy) ---");
    println!("chip:  mean = {:.5}", avg(&chip_above_frac));
    println!("float: mean = {:.5}", avg(&float_above_frac));
    println!(
        "(float should be ~0 by construction -- any real chip excess here is evidence of harmonic extension)"
    );

    println!("\n--- Energy at between-harmonic-line midpoints (noise floor proxy) ---");
    println!("chip:  mean fraction = {:.5}", avg(&chip_between_frac));
    println!("float: mean fraction = {:.5}", avg(&float_between_frac));

    println!("\n--- Per-harmonic magnitude ratio (chip/float), by relative harmonic number k ---");
    for (i, ratios) in ratio_by_relative_harmonic.iter().enumerate() {
        if !ratios.is_empty() {
            println!("k={}: n={}, mean ratio={:.3}", i + 1, ratios.len(), avg(ratios));
        }
    }
}
