// SPDX-License-Identifier: LGPL-3.0-or-later
//! The fixed-point sibling of `ambe_chip_pcm_vs_float_synthesis_ratet27.rs` -- the actual deliverable
//! the standing "finish the fixed-point AMBE implementation, test it against the chip" `/goal` named.
//! Everything up to this harness (the arithmetic primitives, all three modes' parameter dequantize,
//! `unvoiced_synthesis`/`voiced_synthesis`/`SynthesisState` orchestration, and finally
//! `fixed::ratet27::decode::DecoderState`) was prerequisite work; this is the first real comparison of
//! this crate's own *fixed-point* synthesized PCM against the real DVSI chip's own decoded PCM.
//!
//! Runs the float and fixed decoders in the same pass against the same captured chip channel
//! payloads (not two separate runs), so the three-way comparison (chip/float/fixed) comes from one
//! capture and one lag search, and reports fixed-vs-chip envelope correlation directly alongside
//! float-vs-chip's own already-established `~0.63` -- see `night_shift_todo/medium/
//! ambe-fixed-point-port-f0c555a5.md` in hams_com for that number's own history and the two-separate-
//! passes-not-interleaved reasoning this harness's own encode/decode packet ordering follows.
//!
//! **Result, first real run against the live chip (200 real frames, `OSR_us_000_0010_8k.wav`)**:
//! `chip-vs-fixed` frame-level RMS envelope correlation is `0.6293`, matching `chip-vs-float`'s own
//! `0.6293` to four decimal places -- the fixed-point port adds no measurable gap against the chip
//! beyond what the float reference already carries. Direct `fixed-vs-float` envelope correlation is
//! `1.0000`. Sample-level `fixed-vs-float` SNR (phase-sensitive, unlike the envelope numbers above) is
//! `31.4 dB` over the first 8 frames, settling to `~24-25 dB` over the full 200 -- a real but modest
//! decline consistent with the omega0-quantization-mismatch mechanism `tests/ambe_fixed_ratet27_
//! decode.rs`'s own discriminating test already characterized on synthetic frames, here confirmed on
//! genuinely varying real pitch rather than collapsing further (it settles, not compounds
//! exponentially the way the pre-`PI_Q48` bug did). Peak amplitudes are close and well under
//! `voiced_synthesis`'s own `+-32768.0` saturation ceiling (float `28209.1`, fixed `28314.9`), so
//! saturation is not engaging on this material; only `63` of `32000` samples diverge by more than
//! `1000` units between the two decoders.
//!
//! Usage: `cargo run --release --example ambe_chip_pcm_vs_fixed_synthesis_ratet27 -- <host:port>`

use ham_digital_modes::ambe::fixed::ratet27::decode::DecoderState as FixedDecoderState;
use ham_digital_modes::ambe::float::ratet27::decode::DecoderState as FloatDecoderState;
use ham_digital_modes::ambe::float::ratet27::ratet27_wire_format::{block_wire_members, Block};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 200; // 4 seconds at 20ms/frame -- enough for a real lag search plus segments.
const MAX_LAG_SAMPLES: i32 = 2400; // +-15 frames, matching the float harness's own widened window.

fn from_q16_i64(v: i64) -> f64 {
    v as f64 / 65536.0
}

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
/// Sends a previously-captured `TYPE_CHANNEL` payload straight back as an outgoing packet -- the
/// decode-direction capability `p25_ratet27_probe_decode_direction.rs` confirmed works.
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
/// Writes a minimal mono 16-bit 8kHz PCM WAV -- so all three decoded streams can be listened to
/// directly, a far richer diagnostic than any single correlation number.
fn write_wav_mono_i16(path: &str, samples: &[f64]) {
    let pcm: Vec<i16> = samples.iter().map(|&s| s.round().clamp(-32768.0, 32767.0) as i16).collect();
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes()); // byte rate = sample_rate * block_align
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out).unwrap_or_else(|e| panic!("write {path}: {e}"));
}
/// The same raw pre-FEC `c_hat_0..c_hat_7` extraction `ambe_fixed_chip_validate_ratet27.rs` and the
/// float chip-comparison harness both use, confirmed correct there (`Decoded: 3291/3320` real frames).
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

/// Pearson correlation coefficient over two equal-length real slices, `0` if either has zero
/// variance (a genuinely silent segment correlates with nothing meaningfully).
fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let dx = x - mean_a;
        let dy = y - mean_b;
        cov += dx * dy;
        var_a += dx * dx;
        var_b += dy * dy;
    }
    if var_a <= 1e-9 || var_b <= 1e-9 {
        return 0.0;
    }
    cov / (var_a.sqrt() * var_b.sqrt())
}

/// A real lag search (sample-domain cross-correlation) between two streams, returning the best lag
/// and the correlation achieved at it. Shared by both the chip-vs-float and chip-vs-fixed searches so
/// they're computed identically.
fn lag_search(reference: &[f64], candidate: &[f64]) -> (i32, f64) {
    let mut best_lag = 0i32;
    let mut best_corr = f64::NEG_INFINITY;
    let search_len = reference.len().min(candidate.len()) - (2 * MAX_LAG_SAMPLES as usize);
    for lag in -MAX_LAG_SAMPLES..=MAX_LAG_SAMPLES {
        let (ref_slice, cand_slice) = if lag >= 0 {
            (&reference[lag as usize..lag as usize + search_len], &candidate[..search_len])
        } else {
            (&reference[..search_len], &candidate[(-lag) as usize..(-lag) as usize + search_len])
        };
        let c = correlation(ref_slice, cand_slice);
        if c > best_corr {
            best_corr = c;
            best_lag = lag;
        }
    }
    (best_lag, best_corr)
}

/// Direct sample-level SNR between fixed and float PCM (no lag search, no chip involved -- both
/// decoders share the same crate's own frame boundaries, so they're already sample-aligned). Unlike
/// the phase-insensitive envelope correlation above, this is sensitive to phase-accumulator drift: the
/// crate's own synthetic decode tests measured a real (if modest) SNR decline from omega0
/// quantization mismatch compounding over many frames, so this checks whether that same mechanism
/// shows up over 200 real frames of genuinely varying pitch, not just the synthetic sweep.
fn snr_db(float_pcm: &[f64], fixed_pcm: &[f64]) -> f64 {
    let signal_power: f64 = float_pcm.iter().map(|&s| s * s).sum();
    let noise_power: f64 = float_pcm.iter().zip(fixed_pcm.iter()).map(|(&f, &x)| (f - x) * (f - x)).sum();
    if noise_power <= 1e-12 {
        return f64::INFINITY;
    }
    10.0 * (signal_power / noise_power).log10()
}

fn frame_rms(pcm: &[f64]) -> Vec<f64> {
    pcm.chunks(FRAME_SAMPLES)
        .filter(|c| c.len() == FRAME_SAMPLES)
        .map(|c| (c.iter().map(|&s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt())
        .collect()
}

/// Segmental correlation at a given best lag, split by whether the segment looks predominantly
/// voiced (RMS-based proxy, matching the float harness's own classifier exactly) -- returns
/// (voiced_mean, other_mean, voiced_count, other_count).
fn segmental_correlation(chip_pcm: &[f64], candidate_pcm: &[f64], best_lag: i32) -> (f64, f64, usize, usize) {
    let mut voiced_corrs = Vec::new();
    let mut other_corrs = Vec::new();
    let offset = best_lag.unsigned_abs() as usize;
    let usable_frames = ((chip_pcm.len().min(candidate_pcm.len()) - offset) / FRAME_SAMPLES).saturating_sub(1);
    for i in 0..usable_frames {
        let (chip_start, cand_start) = if best_lag >= 0 {
            (offset + i * FRAME_SAMPLES, i * FRAME_SAMPLES)
        } else {
            (i * FRAME_SAMPLES, offset + i * FRAME_SAMPLES)
        };
        let chip_seg = &chip_pcm[chip_start..chip_start + FRAME_SAMPLES];
        let cand_seg = &candidate_pcm[cand_start..cand_start + FRAME_SAMPLES];
        let rms: f64 = (chip_seg.iter().map(|&s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt();
        let corr = correlation(chip_seg, cand_seg);
        if rms > 200.0 {
            voiced_corrs.push(corr);
        } else {
            other_corrs.push(corr);
        }
    }
    let avg = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    (avg(&voiced_corrs), avg(&other_corrs), voiced_corrs.len(), other_corrs.len())
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

    let mut float_decoder = FloatDecoderState::new();
    let mut fixed_decoder = FixedDecoderState::new();
    let mut chip_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut float_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut fixed_pcm: Vec<f64> = Vec::with_capacity(n_frames * FRAME_SAMPLES);
    let mut float_decode_failures = 0usize;
    let mut fixed_decode_failures = 0usize;

    // Two separate passes, not interleaved per frame -- see the float harness's own doc comment for
    // why (the chip's own decoder almost certainly carries frame-to-frame continuity state that an
    // interleaved encode request could silently reset).
    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        channel_payloads.push(payload.to_vec());
    }

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
            None => {
                float_decode_failures += 1;
                float_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES));
            }
        }
        match fixed_decoder.decode_frame(c) {
            Some(frame_pcm) => fixed_pcm.extend(frame_pcm.iter().map(|&s| from_q16_i64(s))),
            None => {
                fixed_decode_failures += 1;
                fixed_pcm.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES));
            }
        }
    }

    println!(
        "Captured {n_frames} real frames' worth of PCM from the chip's own decoder, this crate's \
         float synthesis ({float_decode_failures} failures), and this crate's fixed-point synthesis \
         ({fixed_decode_failures} failures)."
    );

    let (float_lag, float_corr) = lag_search(&chip_pcm, &float_pcm);
    let (fixed_lag, fixed_corr) = lag_search(&chip_pcm, &fixed_pcm);
    println!(
        "chip-vs-float best alignment: lag={float_lag} samples ({:.2} ms), whole-buffer correlation = {float_corr:.4}",
        float_lag as f64 * 1000.0 / 8000.0
    );
    println!(
        "chip-vs-fixed best alignment: lag={fixed_lag} samples ({:.2} ms), whole-buffer correlation = {fixed_corr:.4}",
        fixed_lag as f64 * 1000.0 / 8000.0
    );

    let (float_voiced, float_other, float_voiced_n, float_other_n) =
        segmental_correlation(&chip_pcm, &float_pcm, float_lag);
    let (fixed_voiced, fixed_other, fixed_voiced_n, fixed_other_n) =
        segmental_correlation(&chip_pcm, &fixed_pcm, fixed_lag);
    println!(
        "chip-vs-float higher-energy segments: {float_voiced_n} frames, mean correlation {float_voiced:.4}; \
         lower-energy segments: {float_other_n} frames, mean correlation {float_other:.4}"
    );
    println!(
        "chip-vs-fixed higher-energy segments: {fixed_voiced_n} frames, mean correlation {fixed_voiced:.4}; \
         lower-energy segments: {fixed_other_n} frames, mean correlation {fixed_other:.4}"
    );

    // Frame-level RMS envelope correlation (phase-insensitive, no fine lag applied) -- the acceptance
    // criterion `night_shift_todo/medium/ambe-fixed-point-port-f0c555a5.md` established: fixed should
    // land at the same `~0.63` float already achieves.
    let chip_envelope = frame_rms(&chip_pcm);
    let float_envelope = frame_rms(&float_pcm);
    let fixed_envelope = frame_rms(&fixed_pcm);
    let float_envelope_len = chip_envelope.len().min(float_envelope.len());
    let fixed_envelope_len = chip_envelope.len().min(fixed_envelope.len());
    let chip_vs_float_envelope = correlation(&chip_envelope[..float_envelope_len], &float_envelope[..float_envelope_len]);
    let chip_vs_fixed_envelope = correlation(&chip_envelope[..fixed_envelope_len], &fixed_envelope[..fixed_envelope_len]);
    println!("chip-vs-float frame-level RMS envelope correlation: {chip_vs_float_envelope:.4}");
    println!("chip-vs-fixed frame-level RMS envelope correlation: {chip_vs_fixed_envelope:.4}");

    // Fixed-vs-float directly: the crate's own already-established synthetic-frame comparisons put a
    // floor under this (30+ dB multi-frame SNR against hand-built frames), but this is the first time
    // it's checked against real chip-derived channel bits end to end.
    let direct_envelope_len = float_envelope.len().min(fixed_envelope.len());
    println!(
        "fixed-vs-float frame-level RMS envelope correlation (direct, no chip involved): {:.4}",
        correlation(&float_envelope[..direct_envelope_len], &fixed_envelope[..direct_envelope_len])
    );

    // Sample-level (not envelope) fixed-vs-float SNR, and over what frame-count window -- the
    // envelope correlation above is phase-insensitive by construction and can't see phase-accumulator
    // drift on its own; this can.
    for &n in &[8usize, 50, 200] {
        let len = (n * FRAME_SAMPLES).min(float_pcm.len()).min(fixed_pcm.len());
        println!("fixed-vs-float sample-level SNR over first {n} frames: {:.1} dB", snr_db(&float_pcm[..len], &fixed_pcm[..len]));
    }
    let full_len = float_pcm.len().min(fixed_pcm.len());
    println!(
        "fixed-vs-float sample-level SNR over all {n_frames} frames: {:.1} dB",
        snr_db(&float_pcm[..full_len], &fixed_pcm[..full_len])
    );

    // Peak amplitude and large-divergence sample count, to check whether voiced_synthesis's own i32
    // Q16.16 saturation (+-32768.0 real headroom) is actually engaging on real loud chip-derived
    // frames -- the float harness already recorded float PCM running "several times louder than the
    // chip" on some frames, so this is worth checking directly rather than assuming headroom is fine.
    let peak = |pcm: &[f64]| pcm.iter().map(|&s| s.abs()).fold(0.0, f64::max);
    let large_diff_count = float_pcm
        .iter()
        .zip(fixed_pcm.iter())
        .filter(|&(&f, &x)| (f - x).abs() > 1000.0)
        .count();
    println!(
        "peak |float_pcm| = {:.1}, peak |fixed_pcm| = {:.1}, samples with |float-fixed| > 1000: {large_diff_count}",
        peak(&float_pcm),
        peak(&fixed_pcm)
    );

    let out_dir = std::env::var("AMBE_RATET27_WAV_OUT_DIR").unwrap_or_else(|_| "/tmp".to_string());
    write_wav_mono_i16(&format!("{out_dir}/ratet27_chip_decoded.wav"), &chip_pcm);
    write_wav_mono_i16(&format!("{out_dir}/ratet27_float_decoded.wav"), &float_pcm);
    write_wav_mono_i16(&format!("{out_dir}/ratet27_fixed_decoded.wav"), &fixed_pcm);
    println!(
        "Wrote {out_dir}/ratet27_{{chip,float,fixed}}_decoded.wav for direct listening comparison \
         (set AMBE_RATET27_WAV_OUT_DIR to change the output directory)."
    );
}
