// SPDX-License-Identifier: LGPL-3.0-or-later
//! The root-cause diagnostic behind the fix in `ambe::float::ratet27::decode::DecoderState::
//! decode_parameters`/`encode_code_vectors` (removing a spurious demodulation/modulation step --
//! see those functions' own doc comments for the full story). Kept as a permanent regression/
//! evidence tool, not a throwaway.
//!
//! Three checks, in the order that actually found the bug:
//! 1. **Controlled single-harmonic synthesis sanity check** (bottom of `main`): feeds
//!    `VoicedState::synthesize` a known amplitude/pitch and confirms the output matches Eq. 127's
//!    own literal expectation exactly. This passed from the start -- synthesis itself was never the
//!    problem, which narrowed the search to everything upstream of it.
//! 2. **Pitch cross-check via autocorrelation, chip PCM vs. derived `omega0_tilde`**: an
//!    *inconclusive* check, kept here as a documented cautionary example, not because it worked.
//!    The `pitch(OUR_float_pcm, known_omega0=...)` column is the tell: it frequently disagrees with
//!    the *known-correct* `omega0_tilde` on our own correctly-synthesized PCM (multi-harmonic real
//!    speech defeats naive normalized autocorrelation with sub/super-harmonic picks), which means
//!    the chip-vs-ours pitch comparison alongside it was never valid evidence either way. Don't
//!    trust this column; it's printed for transparency about what was tried and ruled out, not as a
//!    working diagnostic.
//! 3. **The no-demod hypothesis test**: recomputes `b0`/`L~`/voicing directly from `c` with no
//!    demodulation step, alongside `decode_parameters`'s own (formerly demodulated) result. `b0`
//!    turned out identical either way (it depends only on `u0`/`u7`, both never modulated) -- but
//!    voicing differs substantially on most frames, which is what led to checking
//!    `AMBE_CHIP_VALIDATION_FINDINGS.md` section 23 and finding that this codebase's own prior,
//!    already-committed GF(2) rank analysis had already established the real chip never modulates
//!    at all. That's the actual fix, applied directly in `decode.rs`/`mod.rs`, not by branching on
//!    a flag here.
//!
//! Usage: `cargo run --release --example ratet27_diagnose_synthesis_mismatch -- <host:port>`

use ham_digital_modes::ambe::float::ratet27::bit_prioritization::{
    deprioritize_bits, extract_fundamental_frequency_quantizer,
};
use ham_digital_modes::ambe::float::ratet27::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::ratet27::parameter_encoding::{
    decode_voicing_decisions_per_harmonic, dequantize_fundamental_frequency,
};
use ham_digital_modes::ambe::float::ratet27::ratet27_wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::float::ratet27::tables::{gain_bit_allocation, higher_order_bit_allocation};
use ham_digital_modes::ambe::float::ratet27::vuv::{frequency_bands_count, harmonics_count};
use ham_digital_modes::ambe::float::ratet27::voiced_synthesis::VoicedState;
use ham_digital_modes::ambe::float::ratet27::unvoiced_synthesis::NoiseState;
use ham_digital_modes::ambe::general::fec::{golay_decode, hamming_decode};
use std::f64::consts::PI;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 20;

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

/// A simple normalized autocorrelation pitch estimate over a real PCM window: searches lags
/// corresponding to 60-400 Hz (typical voice pitch range at 8kHz: period 20..133 samples), returns
/// `(period_samples, normalized_peak, estimated_hz)`. Uses a window wider than one 20ms frame (pitch
/// periods up to 133 samples need more than 160 samples of context to autocorrelate reliably).
fn estimate_pitch(samples: &[f64]) -> (usize, f64, f64) {
    let energy: f64 = samples.iter().map(|&s| s * s).sum();
    if energy < 1.0 {
        return (0, 0.0, 0.0);
    }
    let min_period = 20usize; // 400 Hz
    let max_period = 133usize; // 60 Hz
    let mut best_period = 0;
    let mut best_norm = 0.0;
    for period in min_period..=max_period.min(samples.len() / 2) {
        let mut num = 0.0;
        let mut den_a = 0.0;
        let mut den_b = 0.0;
        for i in 0..(samples.len() - period) {
            num += samples[i] * samples[i + period];
            den_a += samples[i] * samples[i];
            den_b += samples[i + period] * samples[i + period];
        }
        let denom = (den_a * den_b).sqrt();
        if denom < 1e-9 {
            continue;
        }
        let norm = num / denom;
        if norm > best_norm {
            best_norm = norm;
            best_period = period;
        }
    }
    let hz = if best_period > 0 { 8000.0 / best_period as f64 } else { 0.0 };
    (best_period, best_norm, hz)
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

    let mut float_decoder = DecoderState::new();
    let mut float_decoder_pcm = DecoderState::new();
    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(N_FRAMES);
    for i in 0..N_FRAMES {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        channel_payloads.push(payload.to_vec());
    }

    let mut chip_pcm_all: Vec<f64> = Vec::new();
    let mut float_pcm_all: Vec<f64> = Vec::new();
    for (i, channel_payload) in channel_payloads.iter().enumerate() {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);

        let n = send_recv_retrying(&sock, &mut buf, &build_channel(channel_payload));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH);
        let chip_frame_pcm = parse_speech_payload(payload);
        chip_pcm_all.extend(chip_frame_pcm.iter().map(|&s| s as f64));

        // Estimator-validation control: our OWN float-synthesized PCM, whose true pitch we KNOW
        // (it was synthesized directly from omega0_tilde) -- run the identical autocorrelation
        // estimator on it. If this also disagrees with omega0_hz, the estimator itself is unreliable
        // on this material and the chip-vs-ours pitch comparison below is meaningless noise, not a
        // real bug signal.
        match float_decoder_pcm.decode_frame(c) {
            Some(frame_pcm) => float_pcm_all.extend(frame_pcm.iter().copied()),
            None => float_pcm_all.extend(std::iter::repeat_n(0.0, FRAME_SAMPLES)),
        }

        // Hypothesis test: decode_parameters demodulates c[1..6] via modulate_code_vectors(c, u0)
        // before FEC-decoding them -- but ratet27_fec::decode_block (the path
        // ambe_chip_validate_ratet27.rs's own zero-corrected-error PASS harness confirms against
        // the live chip) FEC-decodes the SAME raw wire-extracted codewords with NO demodulation at
        // all. modulate_code_vectors's own modulation vector is a pseudo-random sequence seeded by
        // u0 (Eq. 84-85) that is nonzero for essentially every index regardless of u0's own value,
        // so demodulation is not a near-no-op -- if the real chip's own wire format doesn't
        // actually need it, decode_parameters's own u1..u6/b0/L would be corrupted on every frame.
        // Compute both variants directly: this uses golay_decode/hamming_decode straight on `c`,
        // skipping modulate_code_vectors entirely, to compare against the pitch the chip's own PCM
        // actually shows.
        {
            let (u0_nodemod, _) = golay_decode(c[0]);
            let (u1, _) = golay_decode(c[1]);
            let (u2, _) = golay_decode(c[2]);
            let (u3, _) = golay_decode(c[3]);
            let (u4, _) = hamming_decode(c[4] as u16);
            let (u5, _) = hamming_decode(c[5] as u16);
            let (u6, _) = hamming_decode(c[6] as u16);
            let u7 = c[7];
            let u_vectors_nodemod: [u32; 8] =
                [u0_nodemod as u32, u1 as u32, u2 as u32, u3 as u32, u4 as u32, u5 as u32, u6 as u32, u7];
            let b0_nodemod = extract_fundamental_frequency_quantizer(&u_vectors_nodemod);
            let omega0_nodemod = dequantize_fundamental_frequency(b0_nodemod);
            let l_nodemod = harmonics_count(omega0_nodemod);
            let omega0_hz_nodemod = omega0_nodemod * 8000.0 / (2.0 * PI);

            // Voicing-fraction hypothesis: b0/L~/omega0 don't depend on demodulation (u0/u7 are
            // never modulated), but per-harmonic voicing (b1) does, since it comes from
            // deprioritize_bits(u_vectors, ...) which consumes u1..u6. Compute voicing under the
            // no-demod u_vectors and compare against what decode_parameters actually produced.
            let k_hat_nodemod = frequency_bands_count(l_nodemod);
            let gain_widths: [u8; 5] =
                std::array::from_fn(|idx| gain_bit_allocation(l_nodemod, idx as u32 + 2).map(|(w, _)| w).unwrap_or(0));
            let higher_widths: Vec<u8> = higher_order_bit_allocation(l_nodemod)
                .map(|w| w.iter().copied().filter(|&w| w > 0).collect())
                .unwrap_or_default();
            let voiced_nodemod = deprioritize_bits(u_vectors_nodemod, k_hat_nodemod, gain_widths, &higher_widths)
                .map(|bits| decode_voicing_decisions_per_harmonic(bits.b1, k_hat_nodemod, l_nodemod))
                .map(|v| v.iter().filter(|&&b| b).count());

            println!(
                "  [no-demod hypothesis] u0={u0_nodemod} b0={b0_nodemod} L~={l_nodemod} \
                 omega0_hz={omega0_hz_nodemod:.1} voiced_nodemod={voiced_nodemod:?}/{l_nodemod}"
            );
        }

        // Check (1): what did OUR decoder derive from this same c?
        match float_decoder.decode_parameters(c) {
            Some(FrameOutcome::Decoded(params)) => {
                float_decoder.advance_history(&params);
                let voiced_count = params.voiced.iter().filter(|&&v| v).count();
                let omega0_hz = params.omega0_tilde * 8000.0 / (2.0 * PI);
                let implied_period = if params.omega0_tilde > 0.0 { 2.0 * PI / params.omega0_tilde } else { 0.0 };
                let amp_sum: f64 = params.reconstructed_amplitudes.iter().sum();
                let amp_max = params.reconstructed_amplitudes.iter().cloned().fold(0.0, f64::max);

                // Check: chip PCM's own actual pitch AND our own float PCM's own actual pitch
                // (estimator-validation control), from a wider window centered on this frame
                // (autocorrelation needs more than 160 samples for periods up to 133 samples).
                let window_start = (i.saturating_sub(1)) * FRAME_SAMPLES;
                let window_end = ((i + 2) * FRAME_SAMPLES).min(chip_pcm_all.len().min(float_pcm_all.len()));
                let (period, norm, hz) = if window_end > window_start {
                    estimate_pitch(&chip_pcm_all[window_start..window_end])
                } else {
                    (0, 0.0, 0.0)
                };
                let (float_period, float_norm, float_hz) = if window_end > window_start {
                    estimate_pitch(&float_pcm_all[window_start..window_end])
                } else {
                    (0, 0.0, 0.0)
                };

                println!(
                    "frame {i}: L~={l_hat} voiced={voiced_count}/{l_hat} omega0_hz={omega0_hz:.1} \
                     (implied_period={implied_period:.1} samples) amp_sum={amp_sum:.1} \
                     amp_max={amp_max:.1}\n    pitch(chip_pcm)={hz:.1}Hz(corr={norm:.2}) \
                     pitch(OUR_float_pcm, known_omega0={omega0_hz:.1}Hz)={float_hz:.1}Hz(corr={float_norm:.2}) \
                     [periods: chip={period} float={float_period} samples]",
                    l_hat = params.l_hat,
                );
            }
            Some(FrameOutcome::Repeat) => println!("frame {i}: Repeat"),
            Some(FrameOutcome::Mute) => println!("frame {i}: Mute"),
            None => println!("frame {i}: decode_parameters returned None"),
        }
    }

    // Check (3): controlled single-harmonic synthesis sanity check -- feed a known amplitude/pitch
    // voiced-only input and confirm the output sine's peak amplitude and frequency match Eq. 127-141's
    // own literal expectation (peak amplitude ~= 2 * input amplitude per Eq. 127's own factor of 2,
    // frequency = omega0/(2*pi)*8000 Hz), isolating synthesis-internal correctness from everything
    // upstream (parameter dequantize, enhancement).
    println!("\n-- Controlled single-harmonic synthesis sanity check --");
    let mut voiced_state = VoicedState::new();
    let noise = NoiseState::new();
    let test_omega0 = 2.0 * PI / 80.0; // period 80 samples = 100 Hz.
    let test_amplitude = 100.0;
    let voiced = vec![true; 1];
    let amplitudes = vec![test_amplitude; 1];
    // Two calls: VoicedState starts with omega0_prev seeded to a different value (Annex A default),
    // so the first call is a (false,true)/onset transition (Eq. 132); the second call, with the same
    // omega0/amplitude held steady, exercises the steady-state (true,true) continuous-phase branch
    // (Eq. 134-135) which is what real sustained voiced speech mostly uses.
    let _ = voiced_state.synthesize(&noise, test_omega0, &voiced, &amplitudes);
    let s_v = voiced_state.synthesize(&noise, test_omega0, &voiced, &amplitudes).unwrap();
    let peak = s_v.iter().cloned().fold(0.0, f64::max);
    let trough = s_v.iter().cloned().fold(0.0, f64::min);
    let (period, norm, hz) = estimate_pitch(&s_v);
    println!(
        "input: single harmonic, amplitude={test_amplitude}, omega0={test_omega0:.6} rad/sample \
         ({:.1} Hz expected)",
        test_omega0 * 8000.0 / (2.0 * PI)
    );
    println!(
        "output: peak={peak:.2} trough={trough:.2} (expected peak ~= {:.1} per Eq. 127's 2x factor), \
         detected period={period} samples ({hz:.1} Hz, norm_corr={norm:.3})",
        2.0 * test_amplitude
    );
}
