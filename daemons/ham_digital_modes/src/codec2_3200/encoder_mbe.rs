// SPDX-License-Identifier: LGPL-3.0-or-later
//! `EncoderMbe`: the 3200 bps encoder with the reference encoder's spectral analysis for pitch and
//! voicing, for hosts with a floating-point unit (the relay's M17 transmitter).
//!
//! **Why.** `EncoderFixed`'s voicing decision is an energy and zero-crossing heuristic with a
//! noise floor that follows every frame it calls unvoiced. On real recorded speech (the four Open
//! Speech Repository sentences in `tests/fixtures/osr_speech`) it calls 1 to 9 percent of frames
//! voiced where Codec2-mod's encoder calls 82 to 97 percent voiced, so a voice sent through it comes
//! out of every decoder as a whisper. Its pitch is the pitch estimator's 3.125 Hz bin, unrefined.
//! Measured in `docs/reviews/M17_CODEC2_WOJCIECH_2026-11-06.md` (hams_com).
//!
//! **What this does instead**, the analysis every Codec2 3200 encoder in the wild uses (the
//! multi-band-excitation analysis of Griffin and Lim, 1988, as David Rowe's Codec2 applies it;
//! Codec2-mod keeps it unchanged): the windowed speech's 512-point spectrum `Sw`; the pitch
//! estimator's fundamental refined by a harmonic-sum search (the candidate period whose harmonics
//! collect the most spectral energy, in 1-sample then quarter-sample steps); per-harmonic
//! amplitudes; and the voicing decision from how well a harmonic model (the window's own spectrum
//! placed at each harmonic, least-squares scaled) explains the spectrum below 1 kHz, a 6 dB
//! signal-to-error threshold, then the low-band/high-band energy-ratio corrections.
//!
//! **Licence.** This is an independent implementation from the method's description, not a
//! translation of the LGPL-2.1-only C (see `vendor/codec2-mod/VENDORED_FROM.md` and LICENSING.md
//! on why this crate does not copy that code). Constants that define the method (the 6 dB
//! threshold, the 1/2/4 kHz band edges, the 10/-10/-4 dB ratio rules, the 60 Hz limit) are the
//! method's parameters.
//!
//! **What stays the same.** The energy and line spectral pairs come from `EncoderFixed`'s integer
//! path unchanged (`EncoderFixed::lpc_fields`), and the bitstream format is the standard one: any
//! Codec2 3200 decoder decodes it.

use super::encoder_fixed::EncoderFixed;
use super::{
    bits, nlp, quantise, window, BYTES_PER_FRAME, E_BITS, FFT_ENC, M_PITCH, NW, N_SAMP, P_MAX,
    P_MIN, SAMPLES_PER_FRAME, SAMPLE_RATE, WO_BITS,
};
use rustfft::num_complex::Complex32;
use std::f32::consts::{PI, TAU};

/// Harmonic model fit below 1 kHz must beat its error by this much (dB) to call a frame voiced.
// [@ANCHOR: mbe_voicing_constants]
const VOICING_SNR_DB: f32 = 6.0;
/// Pitch estimator bin width in Hz (`SAMPLE_RATE / (PE_FFT_SIZE * NLP_DEC)`).
const NLP_BIN_HZ: f32 = SAMPLE_RATE as f32 / (super::PE_FFT_SIZE * super::NLP_DEC) as f32;

/// Pitch and voicing of one 10 ms analysis frame.
// [@ANCHOR: MbeFrame]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MbeFrame {
    /// Fundamental, radians per sample, inside `[TAU/P_MAX, TAU/P_MIN]`.
    pub wo: f32,
    /// Harmonic count below 4 kHz.
    pub l: usize,
    pub voiced: bool,
    /// Harmonic-model signal-to-error ratio below 1 kHz, dB (before the energy-ratio rules).
    pub snr_db: f32,
}

/// The spectral analysis state: the analysis window and its spectrum (computed once).
// [@ANCHOR: MbeAnalyser]
pub struct MbeAnalyser {
    window: [f32; M_PITCH],
    /// Real part of the window's spectrum, rotated so index `FFT_ENC/2` is its DC term.
    window_spectrum: [f32; FFT_ENC],
}

impl Default for MbeAnalyser {
    fn default() -> Self {
        Self::new()
    }
}

impl MbeAnalyser {
    // [@ANCHOR: MbeAnalyser::new]
    pub fn new() -> Self {
        let window = window::make_analysis_window();
        // The window, centred on time zero (its middle at index 0, its first half wrapped to the
        // end), so its spectrum is real; then rotated so DC sits in the middle.
        let mut buf = [Complex32::new(0.0, 0.0); FFT_ENC];
        let mid = M_PITCH / 2;
        for k in 0..NW / 2 {
            buf[k] = Complex32::new(window[mid + k], 0.0);
            buf[FFT_ENC - NW / 2 + k] = Complex32::new(window[mid - NW / 2 + k], 0.0);
        }
        let spec = microfft::complex::cfft_512(&mut buf);
        let half = FFT_ENC / 2;
        let window_spectrum = core::array::from_fn(|i| spec[(i + half) % FFT_ENC].re);
        Self {
            window,
            window_spectrum,
        }
    }

    /// The windowed history's 512-point spectrum, with the window centred on time zero.
    // [@ANCHOR: MbeAnalyser::spectrum]
    pub fn spectrum(&self, sn: &[i16; M_PITCH]) -> [Complex32; FFT_ENC] {
        let mut buf = [Complex32::new(0.0, 0.0); FFT_ENC];
        let mid = M_PITCH / 2;
        for k in 0..NW / 2 {
            buf[k] = Complex32::new(sn[mid + k] as f32 * self.window[mid + k], 0.0);
            let j = mid - NW / 2 + k;
            buf[FFT_ENC - NW / 2 + k] = Complex32::new(sn[j] as f32 * self.window[j], 0.0);
        }
        *microfft::complex::cfft_512(&mut buf)
    }

    /// Pitch and voicing of the frame in `sn`, starting from the pitch estimator's `f0` (Hz).
    // [@ANCHOR: MbeAnalyser::analyse]
    pub fn analyse(&self, sn: &[i16; M_PITCH], f0_hz: f32) -> MbeFrame {
        let sw = self.spectrum(sn);
        let f0 = f0_hz.clamp(
            SAMPLE_RATE as f32 / P_MAX as f32,
            SAMPLE_RATE as f32 / P_MIN as f32,
        );
        let wo = refine_pitch(&sw, TAU * f0 / SAMPLE_RATE as f32);
        let l = harmonic_count(wo);
        let amps = harmonic_amplitudes(&sw, wo, l);
        let snr_db = harmonic_fit_snr_db(&sw, &self.window_spectrum, wo, l, &amps);
        let voiced = apply_energy_ratio_rules(snr_db > VOICING_SNR_DB, wo, l, &amps);
        MbeFrame {
            wo,
            l,
            voiced,
            snr_db,
        }
    }
}

/// Bin of `x` radians per sample in the `FFT_ENC`-point spectrum, as a real number.
// [@ANCHOR: mbe_bin_of]
fn bin_of(x: f32) -> f32 {
    x * FFT_ENC as f32 / TAU
}

/// Harmonics below 4 kHz for fundamental `wo`, one fewer when the last one sits within 5 percent
/// of the band edge (a rounding guard).
// [@ANCHOR: mbe_harmonic_count]
pub fn harmonic_count(wo: f32) -> usize {
    let mut l = (PI / wo) as usize;
    if l > 1 && wo * l as f32 >= 0.95 * PI {
        l -= 1;
    }
    l.max(1)
}

/// Energy the spectrum holds at the harmonic bins of fundamental `wo`, harmonics `1..=l`.
// [@ANCHOR: mbe_harmonic_sum]
fn harmonic_sum(sw: &[Complex32; FFT_ENC], wo: f32, l: usize) -> f32 {
    (1..=l)
        .map(|m| {
            let b = (bin_of(m as f32 * wo) + 0.5) as usize;
            sw.get(b).map_or(0.0, |c| c.norm_sqr())
        })
        .sum()
}

/// Refines `wo` to the period (searched over +-5 samples in 1-sample steps, then +-1 in
/// quarter-sample steps) whose harmonics collect the most spectral energy, then clamps it into the
/// codec's pitch range. The harmonic count stays that of the starting pitch during each search.
// [@ANCHOR: mbe_refine_pitch]
pub fn refine_pitch(sw: &[Complex32; FFT_ENC], wo: f32) -> f32 {
    let mut wo = wo;
    for (reach, step) in [(5.0f32, 1.0f32), (1.0, 0.25)] {
        let l = (PI / wo) as usize;
        let centre = TAU / wo;
        let mut best = (f32::MIN, wo);
        let steps = (2.0 * reach / step).round() as usize;
        for k in 0..=steps {
            let p = centre - reach + k as f32 * step;
            if p <= 0.0 {
                continue;
            }
            let cand = TAU / p;
            let e = harmonic_sum(sw, cand, l);
            if e > best.0 {
                best = (e, cand);
            }
        }
        wo = best.1;
    }
    wo.clamp(TAU / P_MAX as f32, TAU / P_MIN as f32)
}

/// Amplitude of each harmonic `1..=l`: the square root of the spectral energy in its band (half a
/// harmonic either side). Index 0 is unused.
// [@ANCHOR: mbe_harmonic_amplitudes]
pub fn harmonic_amplitudes(sw: &[Complex32; FFT_ENC], wo: f32, l: usize) -> Vec<f32> {
    let mut a = vec![0.0f32; l + 1];
    for (m, am) in a.iter_mut().enumerate().skip(1) {
        let lo = (bin_of((m as f32 - 0.5) * wo) + 0.5) as usize;
        let hi = ((bin_of((m as f32 + 0.5) * wo) + 0.5) as usize).min(FFT_ENC);
        *am = (lo..hi).map(|i| sw[i].norm_sqr()).sum::<f32>().sqrt();
    }
    a
}

/// Signal-to-error ratio (dB) of the voiced harmonic model below 1 kHz: each harmonic's band is
/// fitted by the window spectrum centred on that harmonic, scaled by the least-squares complex
/// gain; the error is what the fit leaves over.
// [@ANCHOR: mbe_harmonic_fit_snr_db]
pub fn harmonic_fit_snr_db(
    sw: &[Complex32; FFT_ENC],
    window_spectrum: &[f32; FFT_ENC],
    wo: f32,
    l: usize,
    amps: &[f32],
) -> f32 {
    let l_1k = ((l as f32 * 1000.0 / (SAMPLE_RATE as f32 / 2.0)) as usize).min(l);
    let signal: f32 = 1e-4 + amps[1..=l_1k].iter().map(|a| a * a).sum::<f32>();
    let mut error = 1e-4f32;
    for h in 1..=l_1k {
        let lo = bin_of((h as f32 - 0.5) * wo).ceil() as usize;
        let hi = (bin_of((h as f32 + 0.5) * wo).ceil() as usize).min(FFT_ENC);
        // Window-spectrum index of bin `i`: `i + shift`, so the harmonic's own bin lands on DC.
        let shift = (FFT_ENC as f32 / 2.0 - bin_of(h as f32 * wo) + 0.5) as isize;
        let w_at = |i: usize| -> f32 {
            let k = i as isize + shift;
            if (0..FFT_ENC as isize).contains(&k) {
                window_spectrum[k as usize]
            } else {
                0.0
            }
        };
        let mut num = Complex32::new(0.0, 0.0);
        let mut den = 0.0f32;
        for (i, s) in sw.iter().enumerate().take(hi).skip(lo) {
            let w = w_at(i);
            num += *s * w;
            den += w * w;
        }
        if den <= 0.0 {
            continue;
        }
        let gain = num / den;
        for (i, s) in sw.iter().enumerate().take(hi).skip(lo) {
            error += (*s - gain * w_at(i)).norm_sqr();
        }
    }
    10.0 * (signal / error).log10()
}

/// The energy-ratio corrections to a voicing decision: a frame with more than 10 dB more energy
/// below 2 kHz than above is voiced; one with 10 dB less is unvoiced; a very low pitch (60 Hz or
/// under) with 4 dB less is unvoiced (a pitch estimate locked on low-frequency noise).
// [@ANCHOR: mbe_apply_energy_ratio_rules]
pub fn apply_energy_ratio_rules(voiced: bool, wo: f32, l: usize, amps: &[f32]) -> bool {
    let l_2k = ((l as f32 * 2000.0 / (SAMPLE_RATE as f32 / 2.0)) as usize).min(l);
    let low: f32 = 1e-4 + amps[1..=l_2k].iter().map(|a| a * a).sum::<f32>();
    let high: f32 = 1e-4 + amps[l_2k.max(1)..=l].iter().map(|a| a * a).sum::<f32>();
    let ratio_db = 10.0 * (low / high).log10();
    if !voiced {
        return ratio_db > 10.0;
    }
    let sixty_hz = TAU * 60.0 / SAMPLE_RATE as f32;
    !(ratio_db < -10.0 || (ratio_db < -4.0 && wo <= sixty_hz))
}

/// The 3200 bps encoder with spectral (multi-band-excitation) pitch and voicing analysis; energy
/// and line spectral pairs as `EncoderFixed`. Needs `std` (floating point, FFT).
// [@ANCHOR: EncoderMbe]
pub struct EncoderMbe {
    core: EncoderFixed,
    analyser: MbeAnalyser,
}

impl Default for EncoderMbe {
    fn default() -> Self {
        Self::new()
    }
}

impl EncoderMbe {
    // [@ANCHOR: EncoderMbe::new]
    pub fn new() -> Self {
        Self {
            core: EncoderFixed::new(),
            analyser: MbeAnalyser::new(),
        }
    }

    /// Encodes one 20 ms frame: two 10 ms analyses (the first gives the first voicing bit, the
    /// second the pitch and the second voicing bit), then the energy and line spectral pairs of the
    /// second.
    // [@ANCHOR: EncoderMbe::encode]
    pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        let first = self.analyse_half(&speech[..N_SAMP]);
        let second = self.analyse_half(&speech[N_SAMP..]);
        let (e_index, lsp_indexes) = self.core.lpc_fields();
        let fields = bits::FrameFields {
            voiced0: first.voiced,
            voiced1: second.voiced,
            wo_index: quantise::encode_wo(second.wo),
            e_index,
            lsp_indexes,
        };
        bits::pack_frame(&fields, WO_BITS, E_BITS)
    }

    // [@ANCHOR: EncoderMbe::analyse_half]
    fn analyse_half(&mut self, half: &[i16]) -> MbeFrame {
        self.core.shift_in(half);
        let bin = nlp::nlp_fixed_bin(&mut self.core.nlp_state, &self.core.sn);
        self.analyser
            .analyse(&self.core.sn, bin as f32 * NLP_BIN_HZ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f0: f32, amps: &[f32], n: usize, t0: usize) -> Vec<i16> {
        (0..n)
            .map(|i| {
                let t = (t0 + i) as f32 / SAMPLE_RATE as f32;
                amps.iter()
                    .enumerate()
                    .map(|(k, a)| a * (TAU * f0 * (k + 1) as f32 * t).sin())
                    .sum::<f32>() as i16
            })
            .collect()
    }

    fn history(samples: &[i16]) -> [i16; M_PITCH] {
        samples[..M_PITCH].try_into().unwrap()
    }

    fn noise(n: usize, scale: f32) -> Vec<i16> {
        let mut seed = 99u64;
        (0..n)
            .map(|_| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (((seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 2.0 * scale) as i16
            })
            .collect()
    }

    // Tests [@ANCHOR: MbeAnalyser::new]
    // Tests [@ANCHOR: MbeAnalyser]
    // Tests [@ANCHOR: MbeAnalyser::spectrum]
    #[test]
    fn window_spectrum_peaks_at_dc_in_the_middle_and_is_symmetric() {
        let a = MbeAnalyser::new();
        let w = &a.window_spectrum;
        let mid = FFT_ENC / 2;
        let peak = (0..FFT_ENC)
            .max_by(|&i, &j| w[i].partial_cmp(&w[j]).unwrap())
            .unwrap();
        assert_eq!(peak, mid);
        for k in 1..20 {
            assert!((w[mid + k] - w[mid - k]).abs() < 1e-6 * w[mid].abs().max(1.0));
        }
        // A pure tone's spectrum peaks at the tone's bin.
        let s = history(&tone(1000.0, &[8000.0], M_PITCH, 0));
        let sw = a.spectrum(&s);
        let b = (0..FFT_ENC / 2)
            .max_by(|&i, &j| sw[i].norm_sqr().partial_cmp(&sw[j].norm_sqr()).unwrap())
            .unwrap();
        assert_eq!(b, 64, "1000 Hz is bin 64 of 512 at 8 kHz");
    }

    // Tests [@ANCHOR: mbe_refine_pitch]
    // Tests [@ANCHOR: mbe_harmonic_sum]
    // Tests [@ANCHOR: mbe_bin_of]
    // Tests [@ANCHOR: mbe_harmonic_count]
    #[test]
    fn pitch_refinement_lands_on_the_true_fundamental_from_a_coarse_start() {
        let a = MbeAnalyser::new();
        for f0 in [87.3f32, 123.0, 161.7, 222.2, 310.0] {
            let s = history(&tone(
                f0,
                &[6000.0, 4000.0, 3000.0, 2000.0, 1000.0],
                M_PITCH,
                7,
            ));
            let sw = a.spectrum(&s);
            let start = TAU * (f0 + 4.0) / SAMPLE_RATE as f32;
            let wo = refine_pitch(&sw, start);
            let got = wo * SAMPLE_RATE as f32 / TAU;
            assert!((got - f0).abs() / f0 < 0.02, "f0 {f0}: refined to {got} Hz");
            let l = harmonic_count(wo);
            let full = (PI / wo) as usize;
            let expect = if full as f32 * wo >= 0.95 * PI {
                full - 1
            } else {
                full
            };
            assert_eq!(l, expect, "f0 {f0}");
        }
        // Clamped into the codec's range.
        let sw = a.spectrum(&[0; M_PITCH]);
        let wo = refine_pitch(&sw, TAU * 30.0 / SAMPLE_RATE as f32);
        assert!(wo >= TAU / P_MAX as f32 && wo <= TAU / P_MIN as f32);
    }

    // Tests [@ANCHOR: MbeAnalyser::analyse]
    // Tests [@ANCHOR: MbeFrame]
    // Tests [@ANCHOR: mbe_voicing_constants]
    // Tests [@ANCHOR: mbe_harmonic_amplitudes]
    // Tests [@ANCHOR: mbe_harmonic_fit_snr_db]
    #[test]
    fn a_harmonic_tone_is_voiced_and_white_noise_is_not() {
        let a = MbeAnalyser::new();
        let voiced = history(&tone(
            140.0,
            &[5000.0, 4000.0, 3000.0, 2500.0, 2000.0, 1500.0],
            M_PITCH,
            0,
        ));
        let f = a.analyse(&voiced, 140.0);
        assert!(f.voiced && f.snr_db > 15.0, "{f:?}");
        let amps = harmonic_amplitudes(&a.spectrum(&voiced), f.wo, f.l);
        assert!(amps[1] > amps[6] && amps[6] > amps[12], "{amps:?}");
        let hiss = history(&noise(M_PITCH, 6000.0));
        let f = a.analyse(&hiss, 140.0);
        assert!(!f.voiced && f.snr_db < VOICING_SNR_DB, "{f:?}");
    }

    // Tests [@ANCHOR: mbe_apply_energy_ratio_rules]
    #[test]
    fn energy_ratio_rules_override_the_fit_at_the_extremes() {
        let wo = TAU * 100.0 / SAMPLE_RATE as f32;
        let l = harmonic_count(wo);
        let mut low_heavy = vec![0.0f32; l + 1];
        let mut high_heavy = vec![0.0f32; l + 1];
        for m in 1..=l {
            let f = m as f32 * 100.0;
            low_heavy[m] = if f < 2000.0 { 1000.0 } else { 10.0 };
            high_heavy[m] = if f < 2000.0 { 10.0 } else { 1000.0 };
        }
        assert!(apply_energy_ratio_rules(false, wo, l, &low_heavy));
        assert!(!apply_energy_ratio_rules(true, wo, l, &high_heavy));
        let flat = vec![1.0f32; l + 1];
        assert!(apply_energy_ratio_rules(true, wo, l, &flat));
        assert!(!apply_energy_ratio_rules(false, wo, l, &flat));
        // A 55 Hz "pitch" with 5 dB more high-band energy: unvoiced.
        let wo55 = TAU * 55.0 / SAMPLE_RATE as f32;
        let l55 = harmonic_count(wo55);
        let mut tilt = vec![0.0f32; l55 + 1];
        for (m, t) in tilt.iter_mut().enumerate().skip(1) {
            *t = if m as f32 * 55.0 < 2000.0 { 1.0 } else { 1.8 };
        }
        assert!(!apply_energy_ratio_rules(true, wo55, l55, &tilt));
        assert!(apply_energy_ratio_rules(true, wo, l, &flat));
    }

    // Tests [@ANCHOR: EncoderMbe::new]
    // Tests [@ANCHOR: EncoderMbe]
    // Tests [@ANCHOR: EncoderMbe::encode]
    // Tests [@ANCHOR: EncoderMbe::analyse_half]
    // Tests [@ANCHOR: EncoderFixed::lpc_fields]
    #[test]
    fn encoder_mbe_marks_a_steady_vowel_voiced_and_keeps_the_fixed_encoders_envelope() {
        let speech = tone(
            130.0,
            &[6000.0, 5000.0, 3000.0, 2000.0, 1200.0, 800.0],
            40 * SAMPLES_PER_FRAME,
            0,
        );
        let mut mbe = EncoderMbe::new();
        let mut fixed = EncoderFixed::new();
        let mut voiced = 0;
        for (k, c) in speech.as_chunks::<SAMPLES_PER_FRAME>().0.iter().enumerate() {
            let frame: &[i16; SAMPLES_PER_FRAME] = c;
            let a = bits::unpack_frame(&mbe.encode(frame), WO_BITS, E_BITS);
            let b = bits::unpack_frame(&fixed.encode(frame), WO_BITS, E_BITS);
            // Same history, same integer LPC path: identical energy and spectral envelope.
            assert_eq!(a.e_index, b.e_index, "frame {k}");
            assert_eq!(a.lsp_indexes, b.lsp_indexes, "frame {k}");
            if k >= 2 {
                voiced += a.voiced0 as usize + a.voiced1 as usize;
                let f0 = quantise::decode_wo(a.wo_index) * SAMPLE_RATE as f32 / TAU;
                assert!((f0 - 130.0).abs() < 4.0, "frame {k}: {f0} Hz");
            }
        }
        assert_eq!(voiced, 2 * 38);
    }
}
