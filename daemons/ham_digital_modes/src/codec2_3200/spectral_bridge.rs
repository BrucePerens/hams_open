// SPDX-License-Identifier: LGPL-3.0-or-later
//! Spectral Bridge: an opt-in 16kHz decode extension that extrapolates
//! harmonic amplitudes above the codec's own native ~4kHz ceiling and
//! synthesizes them into the newly-available high-frequency band --
//! Bruce's own explicit design, verbatim: "look at the amplitude of
//! the highest harmonic reproduced by the CODEC, and guess that there
//! would be additional harmonics after it." A real, well-established
//! technique (the same family as AMR-NB->WB, spectral-band-
//! replication) applied to Codec2's own explicit sinusoidal harmonic
//! model rather than to a raw spectral envelope, since Codec2 already
//! reports its own harmonic amplitudes and fundamental directly -- no
//! need to re-infer them the way the original SSB/FM proposal
//! (`SPECTRAL_BRIDGE_AND_COMB_SQUELCH.md`) would have had to.
//!
//! **Why 16kHz, not more 8kHz code.** `Model::new`'s own harmonic-count
//! formula, `l = pi/wo`, means the topmost synthesized harmonic already
//! sits at ~4kHz -- this codec's own Nyquist frequency -- for *every*
//! real `Wo`: confirmed directly, `Wo=W0_MIN` gives `l=80=MAX_AMP`, and
//! `80*50Hz=4000Hz` exactly. There is no unused frequency room to place
//! new harmonics into at 8kHz; anything "after" the last one aliases.
//! This module instead reruns the *same* additive-sinusoid-via-IFFT
//! synthesis `synthesis.rs` already uses (place each harmonic's complex
//! amplitude into its own FFT bin, mirror to a Hermitian-symmetric
//! spectrum, inverse FFT, Parzen-window, overlap-add), at double
//! FFT/sample-rate scale (`FFT_ENC_SB`=1024, `SAMPLE_RATE_SB`=16000).
//! `FFT_ENC_SB` is exactly `2*FFT_ENC`, chosen specifically so the
//! Hz-per-bin resolution is unchanged (8000/512 == 16000/1024 ==
//! 15.625 Hz/bin) -- a harmonic's bin index is then `round(m*wo/fft_r)`
//! with the *same* `fft_r = TAU/FFT_ENC` (512-point) constant
//! `synthesis.rs` already uses, just no longer clamped to `FFT_ENC/2`,
//! so `m > l`'s own bins land correctly in the newly-available upper
//! half without any new scaling constant.
//!
//! **What's reused unchanged vs. new**: harmonics `1..=l` reuse
//! `model.a[m]`/`model.phi[m]` exactly as the normal 8kHz decode
//! already computed them (including that path's own postfilter
//! decision) -- this module never re-derives them. Harmonics
//! `l+1..=l2` (`l2 = min(2*l, MAX_AMP_SB)`) are new: their amplitude
//! comes from `extrapolate_amplitudes` below, and (since extrapolation
//! only ever runs for voiced sub-frames -- see that function's own doc
//! comment) their phase reuses the *same* deterministic voiced
//! excitation-phase formula `synthesis.rs::synthesize_phase` uses for
//! its own unresolved harmonics (`phi0*m`, tracking the same
//! fundamental phase) -- there's no real envelope *phase* information
//! above 4kHz to reuse, only a magnitude estimate, so this module
//! doesn't invent one. There is no unvoiced counterpart to reuse here:
//! an unvoiced sub-frame never reaches this branch at all.
//!
//! **Amplitude extrapolation**: an ordinary least-squares fit of
//! `ln(a[m])` against harmonic frequency over the top `min(l, 16)`
//! known harmonics (closest to, and most representative of, the real
//! high-frequency trend), extrapolated linearly in log-amplitude
//! (equivalently, exponential decay vs. frequency) out to `l2`. A
//! rising fitted trend is clamped flat (`beta.min(0.0)`) rather than
//! extrapolated, since a natural voice spectrum doesn't get *louder*
//! toward its own high-frequency tail and an unclamped rising fit
//! would be a real, audible defect, not a plausible reconstruction.
//!
//! **Fixed-point port**: `extrapolate_amplitudes_fixed`/
//! `SpectralBridgeStateFixed` below, added as a follow-on once this
//! float v1 was validated -- matching this project's own established
//! "float reference first, fixed-point port as an explicit follow-on"
//! discipline (see `codec2_3200::encoder_fixed`'s own doc comment for
//! the precedent). Base-2 log/exp (`fixed_point::log2_q23`/`exp2_q23`)
//! replace the float fit's natural log throughout -- the same linear
//! regression, just scaled by `ln(2)`, which cancels out entirely since
//! both `alpha`/`beta` and their later use in `exp2_q23` stay in the
//! same base-2 domain end to end. The fit's own `x` (harmonic
//! frequency) is kept as a plain integer Hz, not Q23-scaled, so the OLS
//! accumulators (`sum_xx` in particular) can't overflow `i64` -- see
//! `extrapolate_amplitudes_fixed`'s own doc comment.
//!
//! **Simplified relative to the base 8kHz path, and why**: the
//! extrapolated harmonics skip `postfilter_step`'s own voiced/noise
//! reclassification -- there's no real basis for re-deciding
//! voiced/noise on amplitude values this module itself invented, only
//! for the original, really-decoded ones. A real follow-on, not
//! attempted here: perceptual tuning of the extrapolated band (a
//! gentler rolloff, unvoiced noise texture blended in) once this v1's
//! basic approach is validated against real listening, not just the
//! self-consistency checks this module's own tests run -- there is no
//! real captured 16kHz reference to validate against the way every
//! other stage in this codec was, since the extrapolated content is,
//! by construction, this module's own best guess, not a real
//! transmitted or recoverable signal.
//!
//! **Enable/disable switch**: `SpectralBridgeState::enabled`, on by
//! default per Bruce's own explicit direction. When disabled,
//! `synthesize_subframe_sb` still runs (still produces genuine 16kHz
//! output, still reuses harmonics `1..=l` unchanged) but skips harmonic
//! extrapolation entirely (`l2 = l`), leaving the newly-available
//! 4-8kHz band silent -- a real, if less interesting, "upsample with
//! no invented content" mode, not a different code path to maintain.
//! Extrapolation is also always skipped for unvoiced sub-frames even
//! when enabled -- see `extrapolate_amplitudes`'s own doc comment.
//!
//! **2026-11-07, the envelope-driven high band**: `Decoder::decode_16k`
//! now calls `synthesize_subframe_sb_from_envelope`, which takes the slope
//! above 4 kHz from the decoded LPC envelope (`envelope_slope_ln_per_hz`)
//! instead of the postfiltered harmonic amplitudes, and extends unvoiced
//! sub-frames too, as noise (`extrapolate_amplitudes_from_envelope`). On
//! real 16 kHz speech it brings the 4-8 kHz band from 22.6 dB (unvoiced) and
//! 10.1 dB (voiced) below the original to about 5 dB below (README,
//! "The envelope-driven high band"). `extrapolate_amplitudes` above is
//! unchanged (the 1600 bit/s decoder uses it). The fixed-point 3200 decoder
//! has the same method in integer form (`envelope_slope_log2_per_bin_fixed`,
//! `extrapolate_amplitudes_from_envelope_fixed`,
//! `synthesize_subframe_sb_from_envelope_fixed`).

#[cfg(feature = "std")]
use super::envelope::Model;
use super::envelope::ModelFixed;
use super::fixed_fft::{rshift_round_i128, ComplexQ23, SparseInverse};
use super::fixed_point::{exp2_q23, log2_q23};
#[cfg(feature = "std")]
use super::synthesis::ear_protection;
use super::synthesis::{overlap_add_subframe, phase_increment_q32};
use super::trig_fixed::sin_cos_q23;
use super::{FFT_ENC, MAX_AMP, N_SAMP, SAMPLE_RATE};
#[cfg(feature = "std")]
use rustfft::num_complex::Complex32;

const FRAC_BITS: u32 = 23;

/// Doubled bandwidth (16kHz) relative to the base codec's own 8kHz.
pub const SAMPLE_RATE_SB: u32 = 2 * SAMPLE_RATE;
/// One 10ms sub-frame at `SAMPLE_RATE_SB` -- still 10ms of real time
/// per sub-frame, just twice the samples (matches the base codec's own
/// `N_SAMP` cadence, not a separate framing decision).
pub const N_SAMP_SB: usize = 2 * N_SAMP;
pub const SAMPLES_PER_FRAME_SB: usize = 2 * N_SAMP_SB;
pub const TW_SB: usize = 2 * super::TW;
/// `2*FFT_ENC`, chosen specifically to keep Hz-per-bin unchanged (see
/// this module's own doc comment) -- not an independent tuning choice.
pub const FFT_ENC_SB: usize = 2 * FFT_ENC;
/// `2*MAX_AMP`: the most harmonics `extrapolate_amplitudes` could ever
/// need room for (`l2 = min(2*l, MAX_AMP_SB)`, and `l` itself is
/// already capped at `MAX_AMP`).
pub const MAX_AMP_SB: usize = 2 * MAX_AMP;

#[cfg(feature = "std")]
// [@ANCHOR: make_synthesis_window_sb]
pub(crate) fn make_synthesis_window_sb() -> [f32; SAMPLES_PER_FRAME_SB] {
    let mut pn = [0.0f32; SAMPLES_PER_FRAME_SB];
    let n0 = N_SAMP_SB / 2;
    let n1 = 3 * N_SAMP_SB / 2;
    let inv_2tw = 1.0 / (2.0 * TW_SB as f32);
    for (i, v) in pn
        .iter_mut()
        .enumerate()
        .take((n0 + TW_SB).min(SAMPLES_PER_FRAME_SB))
        .skip(n0.saturating_sub(TW_SB))
    {
        *v = (i as f32 - (n0 - TW_SB) as f32) * inv_2tw;
    }
    for v in pn
        .iter_mut()
        .take(n1.saturating_sub(TW_SB))
        .skip(n0 + TW_SB)
    {
        *v = 1.0;
    }
    for (i, v) in pn
        .iter_mut()
        .enumerate()
        .take((n1 + TW_SB).min(SAMPLES_PER_FRAME_SB))
        .skip(n1.saturating_sub(TW_SB))
    {
        *v = ((n1 + TW_SB) as f32 - i as f32) * inv_2tw;
    }
    pn
}

#[cfg(feature = "std")]
/// Extrapolates harmonic amplitudes above `model.l` -- see this
/// module's own doc comment for the fitting method. Returns the
/// extended amplitude array (indices `1..=l2` populated, matching
/// `model.a`'s own convention of leaving index 0 unused) and `l2`
/// itself, the new harmonic count (`model.l` unchanged if `enabled` is
/// false, `model.voiced` is false, or `model.l` is already at
/// `MAX_AMP_SB/2`, i.e. no room to extrapolate into).
///
/// **Voiced-only, deliberately**: the harmonic-series continuation
/// this function computes is well-justified for voiced speech, which
/// really has that harmonic structure -- unvoiced/fricative sounds
/// (s, sh, f) are physically broadband noise, not harmonics, and
/// extrapolating a tonal series into their own high-frequency "sizzle"
/// region has no real listening validation behind it yet. Left off for
/// unvoiced content in this v1 rather than guessed; a real, separate
/// noise-texture treatment for that case is a genuine follow-on, not
/// attempted here.
// [@ANCHOR: extrapolate_amplitudes]
pub fn extrapolate_amplitudes(model: &Model, enabled: bool) -> ([f32; MAX_AMP_SB + 1], usize) {
    let l = model.l;
    let mut a_ext = [0.0f32; MAX_AMP_SB + 1];
    a_ext[1..=l].copy_from_slice(&model.a[1..=l]);

    let l2 = if enabled && model.voiced {
        (2 * l).min(MAX_AMP_SB)
    } else {
        l
    };
    if l2 <= l {
        return (a_ext, l);
    }

    let freq_hz = |m: usize| m as f32 * model.wo * SAMPLE_RATE as f32 / std::f32::consts::TAU;

    let k = l.min(16);
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_xx = 0.0f64;
    let mut sum_xy = 0.0f64;
    let n = k as f64;
    for m in (l - k + 1)..=l {
        let a = model.a[m].max(1e-6);
        let x = freq_hz(m) as f64;
        let y = (a as f64).ln();
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let denom = n * sum_xx - sum_x * sum_x;
    let (alpha, beta) = if denom.abs() > 1e-9 {
        let beta = (n * sum_xy - sum_x * sum_y) / denom;
        let alpha = (sum_y - beta * sum_x) / n;
        (alpha, beta)
    } else {
        (sum_y / n, 0.0)
    };
    // A rising trend is non-physical past the known band -- a natural
    // voice spectrum doesn't get louder toward its own high-frequency
    // tail. Clamp to a flat continuation rather than extrapolate an
    // ever-louder tail.
    let beta = beta.min(0.0);

    #[allow(clippy::needless_range_loop)]
    for m in (l + 1)..=l2 {
        let x = freq_hz(m) as f64;
        let predicted = alpha + beta * x;
        a_ext[m] = predicted.exp() as f32;
    }
    (a_ext, l2)
}

#[cfg(feature = "std")]
/// The decoded spectral envelope's slope across its upper half (2 to 4 kHz), in natural-log
/// amplitude per Hz, never rising: an ordinary least-squares fit of `ln(1/|A(w)|)` against
/// frequency over the bins of `aw` (the LPC analysis filter's spectrum, `envelope::
/// compute_harmonic_amplitudes`'s return value, 15.625 Hz per bin) from 2000 Hz to the last
/// bin. The envelope is smooth where harmonic amplitudes are not (they carry the postfilter's
/// per-harmonic changes and the quantised envelope's sampling at a few points), so its slope is
/// a steadier guess for the band above 4 kHz. An `aw` with fewer than two bins above 2 kHz, or
/// a degenerate fit, gives 0 (flat).
// [@ANCHOR: envelope_slope_ln_per_hz]
pub fn envelope_slope_ln_per_hz(aw: &[Complex32]) -> f32 {
    let hz_per_bin = SAMPLE_RATE as f64 / FFT_ENC as f64;
    let first = (2000.0 / hz_per_bin).ceil() as usize;
    if aw.len() < first + 2 {
        return 0.0;
    }
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0.0f64, 0.0, 0.0, 0.0, 0.0);
    for (i, c) in aw.iter().enumerate().skip(first) {
        let x = i as f64 * hz_per_bin;
        let y = -0.5 * ((c.norm_sqr() as f64) + 1e-12).ln();
        n += 1.0;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-9 {
        return 0.0;
    }
    (((n * sxy - sx * sy) / denom) as f32).min(0.0)
}

#[cfg(feature = "std")]
/// The high band from the decoded envelope: harmonics `l+1 ..= min(2l, MAX_AMP_SB)` for a voiced
/// **or unvoiced** sub-frame, their log amplitude continuing from the mean log amplitude of the
/// top `min(l, 16)` known harmonics (at their mean frequency) with slope `slope_ln_per_hz`
/// (`envelope_slope_ln_per_hz`, never rising). Returns the extended amplitudes (index 0 unused,
/// `1..=l` copied from `model.a`) and the new count; with `enabled` false, or no room above `l`,
/// the count stays `model.l`. Fricatives (s, sh, f), which carry most of their energy above
/// 4 kHz, get a noise band this way; `extrapolate_amplitudes` (the 1600 bit/s decoder's, and the
/// fixed-point path's) extends voiced frames only.
// [@ANCHOR: extrapolate_amplitudes_from_envelope]
pub fn extrapolate_amplitudes_from_envelope(
    model: &Model,
    slope_ln_per_hz: f32,
    enabled: bool,
) -> ([f32; MAX_AMP_SB + 1], usize) {
    let l = model.l;
    let mut a_ext = [0.0f32; MAX_AMP_SB + 1];
    a_ext[1..=l].copy_from_slice(&model.a[1..=l]);
    let l2 = if enabled { (2 * l).min(MAX_AMP_SB) } else { l };
    if l2 <= l {
        return (a_ext, l);
    }
    let freq_hz =
        |m: usize| m as f64 * model.wo as f64 * SAMPLE_RATE as f64 / std::f64::consts::TAU;
    let k = l.min(16);
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    for m in (l - k + 1)..=l {
        sx += freq_hz(m);
        sy += (model.a[m].max(1e-6) as f64).ln();
    }
    let (x0, y0) = (sx / k as f64, sy / k as f64);
    let slope = slope_ln_per_hz.min(0.0) as f64;
    #[allow(clippy::needless_range_loop)]
    for m in (l + 1)..=l2 {
        a_ext[m] = (y0 + slope * (freq_hz(m) - x0)).exp() as f32;
    }
    (a_ext, l2)
}

#[cfg(feature = "std")]
/// Persistent per-decoder Spectral Bridge synthesis state -- parallel
/// to, and independent of, `synthesis::SynthesisState` (the base 8kHz
/// path is untouched by this module entirely). `ex_phase` is updated
/// with the *exact* formula `synthesize_phase` uses (`wo * N_SAMP`,
/// the *original* `N_SAMP`, not `N_SAMP_SB` -- phase is a physical,
/// sample-rate-independent quantity, and both paths advance it once
/// per 10ms sub-frame from the same `Wo`), so harmonics `1..=l`'s own
/// already-decoded phase and this module's own extrapolated harmonics'
/// phase stay consistent with each other from a shared, if
/// independently tracked, phase origin.
pub struct SpectralBridgeState {
    pub enabled: bool,
    sn_: [f32; SAMPLES_PER_FRAME_SB],
    parzen: [f32; SAMPLES_PER_FRAME_SB],
    ex_phase: f32,
    /// Phase source for the unvoiced high band (`synthesize_subframe_sb_from_envelope`); its own
    /// state, never shared with the base synthesis, so the 8 kHz output is unchanged.
    rng: u32,
    /// `microfft::inverse::ifft_1024` (no stored FFT plan needed) is
    /// normalized (divides by N); `synthesize_subframe_sb` below
    /// multiplies its result by `FFT_ENC_SB` to preserve this module's
    /// existing unnormalized-IFFT convention -- see `synthesis.rs`'s
    /// own identical pattern and `Cargo.toml`'s comment on the
    /// `microfft` dependency for the verification this relies on.
    ifft_buf: [Complex32; FFT_ENC_SB],
}

#[cfg(feature = "std")]
impl Default for SpectralBridgeState {
    fn default() -> Self {
        SpectralBridgeState {
            enabled: true,
            sn_: [0.0; SAMPLES_PER_FRAME_SB],
            parzen: make_synthesis_window_sb(),
            ex_phase: 0.0,
            rng: 0x5B_1D6E,
            ifft_buf: [Complex32::new(0.0, 0.0); FFT_ENC_SB],
        }
    }
}

#[cfg(feature = "std")]
impl SpectralBridgeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Synthesizes one 10ms (`N_SAMP_SB`-sample) sub-frame at
    /// `SAMPLE_RATE_SB`, given the *already-decoded* base `model`
    /// (unchanged, same object the normal 8kHz decode produced).
    // [@ANCHOR: SpectralBridgeState::synthesize_subframe_sb]
    pub fn synthesize_subframe_sb(&mut self, model: &Model) -> [i16; N_SAMP_SB] {
        let (a_ext, l2) = extrapolate_amplitudes(model, self.enabled);
        self.synthesize_extended(model, &a_ext, l2)
    }

    /// As `synthesize_subframe_sb`, but the band above 4 kHz follows the decoded envelope's slope
    /// (`envelope_slope_ln_per_hz` of `aw`, the LPC spectrum `envelope::compute_harmonic_amplitudes`
    /// returned for this sub-frame) and unvoiced sub-frames get it too, with random phases
    /// (`extrapolate_amplitudes_from_envelope`). `codec2_3200::Decoder::decode_16k` uses this one.
    /// Harmonics `1..=l` are the base decode's, unchanged; with `enabled` false the result is the
    /// same as `synthesize_subframe_sb`'s.
    // [@ANCHOR: SpectralBridgeState::synthesize_subframe_sb_from_envelope]
    pub fn synthesize_subframe_sb_from_envelope(
        &mut self,
        model: &Model,
        aw: &[Complex32],
    ) -> [i16; N_SAMP_SB] {
        let (a_ext, l2) =
            extrapolate_amplitudes_from_envelope(model, envelope_slope_ln_per_hz(aw), self.enabled);
        self.synthesize_extended(model, &a_ext, l2)
    }

    /// The synthesis shared by both entry points: harmonics `1..=l` as decoded, `l+1..=l2` from
    /// `a_ext` with the voiced excitation phase (voiced) or a random phase (unvoiced).
    // [@ANCHOR: SpectralBridgeState::synthesize_extended]
    fn synthesize_extended(
        &mut self,
        model: &Model,
        a_ext: &[f32; MAX_AMP_SB + 1],
        l2: usize,
    ) -> [i16; N_SAMP_SB] {
        self.ex_phase += model.wo * N_SAMP as f32;
        self.ex_phase -=
            std::f32::consts::TAU * (self.ex_phase / std::f32::consts::TAU + 0.5).floor();
        let phi0 = self.ex_phase;

        self.sn_.copy_within(N_SAMP_SB.., 0);
        self.sn_[N_SAMP_SB - 1] = 0.0;

        for c in self.ifft_buf.iter_mut() {
            *c = Complex32::new(0.0, 0.0);
        }
        let fft_r = std::f32::consts::TAU / FFT_ENC as f32;
        #[allow(clippy::needless_range_loop)]
        for m in 1..=l2 {
            let b = (((m as f32 * model.wo / fft_r) + 0.5) as usize).min(FFT_ENC_SB / 2 - 1);
            self.ifft_buf[b] = if m <= model.l {
                // Reuse the original decode's own already-filtered,
                // already-postfiltered phase/amplitude unchanged.
                model.phi[m] * model.a[m]
            } else if model.voiced {
                // No real envelope phase exists above 4kHz, only the
                // deterministic voiced excitation phase (same formula as
                // the base path's own synthesize_phase).
                let (s, c) = (phi0 * m as f32).sin_cos();
                Complex32::new(c, s) * a_ext[m]
            } else {
                // Unvoiced (only `extrapolate_amplitudes_from_envelope`
                // extends these): noise, an independent random phase per
                // harmonic, as the base path gives unvoiced harmonics.
                let (s, c) = super::synthesis::next_rand(&mut self.rng).sin_cos();
                Complex32::new(c, s) * a_ext[m]
            };
        }
        for k in 1..(FFT_ENC_SB / 2) {
            self.ifft_buf[FFT_ENC_SB - k] = self.ifft_buf[k].conj();
        }

        for c in microfft::inverse::ifft_1024(&mut self.ifft_buf) {
            *c *= FFT_ENC_SB as f32;
        }

        #[allow(clippy::needless_range_loop)]
        for i in 0..(N_SAMP_SB - 1) {
            self.sn_[i] += self.ifft_buf[FFT_ENC_SB - N_SAMP_SB + 1 + i].re * self.parzen[i];
        }
        #[allow(clippy::needless_range_loop)]
        for j in 0..(N_SAMP_SB + 1) {
            let idx = N_SAMP_SB - 1 + j;
            if idx < SAMPLES_PER_FRAME_SB {
                self.sn_[idx] = self.ifft_buf[j].re * self.parzen[N_SAMP_SB - 1 + j];
            }
        }

        let mut out: [f32; N_SAMP_SB] = core::array::from_fn(|i| self.sn_[i]);
        ear_protection(&mut out);

        core::array::from_fn(|i| out[i].clamp(-32767.0, 32767.0) as i16)
    }
}

fn hz_per_rad_q23() -> i64 {
    super::tables::SB_HZ_PER_RAD_Q23
}

/// `freq_hz(m)` (see the float `extrapolate_amplitudes`'s own closure)
/// as a plain integer Hz value, deliberately *not* Q23-scaled --
/// `extrapolate_amplitudes_fixed`'s OLS fit uses this as its own `x`,
/// and keeping it a small plain integer (at most `SAMPLE_RATE_SB/2`)
/// rather than Q23-scaled keeps every accumulator (`sum_xx` most of
/// all) comfortably within `i64` with no widening needed there.
// [@ANCHOR: freq_hz_fixed]
fn freq_hz_fixed(m: usize, wo_q23: i64) -> i64 {
    let mw_q23 = m as i64 * wo_q23;
    let freq_hz_q23 = rshift_round_i128(mw_q23 as i128 * hz_per_rad_q23() as i128, FRAC_BITS);
    rshift_round_i128(freq_hz_q23 as i128, FRAC_BITS)
}

/// Round-to-nearest division for the OLS fit's own two divisions
/// (`beta`, `alpha`) -- same shape `lpc.rs`'s own `div_round_i128`
/// establishes (that one keeps the bare `div_round_i128` anchor; this
/// copy is anchored `spectral_bridge_div_round_i128` so the two are
/// distinct), duplicated locally rather than threaded through as
/// `pub(crate)` (matching this project's own precedent for small,
/// self-contained per-module helpers, e.g. `codec2_1600::fallback_lsp`).
/// `d` is always positive here: an OLS `denom` is a variance sum
/// (`n*sum_xx - sum_x^2 >= 0` by Cauchy-Schwarz) and `n` is a plain
/// positive harmonic count.
// [@ANCHOR: spectral_bridge_div_round_i128]
fn div_round_i128(n: i128, d: i128) -> i64 {
    debug_assert!(d > 0, "div_round_i128: divisor must be positive, got {d}");
    let half = d / 2;
    (if n >= 0 {
        (n + half) / d
    } else {
        (n - half) / d
    }) as i64
}

/// Fixed-point sibling of `extrapolate_amplitudes` -- see this module's
/// own doc comment for the base-2-log reasoning. `y` (log2-amplitude)
/// stays Q23 throughout (`log2_q23`'s own output format), so `beta`
/// comes out of the fit already in "Q23 log2-amplitude per Hz" units:
/// multiplying it by a plain integer Hz `x` again needs no further
/// rescale to get a Q23 result, and neither does adding it to `alpha`
/// (also Q23) -- unlike a Q23*Q23 product, there's no `FRAC_BITS`
/// shift anywhere in this function.
// [@ANCHOR: extrapolate_amplitudes_fixed]
pub(crate) fn extrapolate_amplitudes_fixed(
    model: &ModelFixed,
    enabled: bool,
) -> ([i64; MAX_AMP_SB + 1], usize) {
    let l = model.l;
    let mut a_ext = [0i64; MAX_AMP_SB + 1];
    a_ext[1..=l].copy_from_slice(&model.a[1..=l]);

    let l2 = if enabled && model.voiced {
        (2 * l).min(MAX_AMP_SB)
    } else {
        l
    };
    if l2 <= l {
        return (a_ext, l);
    }

    let k = l.min(16);
    let mut sum_x: i64 = 0;
    let mut sum_y: i64 = 0;
    let mut sum_xx: i64 = 0;
    let mut sum_xy: i64 = 0;
    for m in (l - k + 1)..=l {
        let a_q23 = model.a[m].max(1);
        let x = freq_hz_fixed(m, model.wo);
        let y = log2_q23(a_q23);
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let n = k as i64;
    let denom = n * sum_xx - sum_x * sum_x;
    let (alpha_q23, beta_q23_per_hz) = if denom != 0 {
        let beta = div_round_i128((n * sum_xy - sum_x * sum_y) as i128, denom as i128);
        let alpha = div_round_i128((sum_y - beta * sum_x) as i128, n as i128);
        (alpha, beta)
    } else {
        (div_round_i128(sum_y as i128, n as i128), 0)
    };
    // Same non-rising clamp the float version applies, for the same
    // reason (see extrapolate_amplitudes's own doc comment).
    let beta_q23_per_hz = beta_q23_per_hz.min(0);

    #[allow(clippy::needless_range_loop)]
    for m in (l + 1)..=l2 {
        let x = freq_hz_fixed(m, model.wo);
        let delta_q23 = beta_q23_per_hz as i128 * x as i128;
        debug_assert!(
            delta_q23 >= i64::MIN as i128 && delta_q23 <= i64::MAX as i128,
            "extrapolate_amplitudes_fixed: beta*x={delta_q23} doesn't fit i64"
        );
        let predicted_q23 = alpha_q23 + delta_q23 as i64;
        a_ext[m] = exp2_q23(predicted_q23);
    }
    (a_ext, l2)
}

/// Fixed-point `extrapolate_amplitudes_from_envelope`: harmonics `l+1 ..= min(2l, MAX_AMP_SB)` for a
/// voiced or unvoiced sub-frame, their base-2 log amplitude continuing from the mean `log2_q23` of
/// the top `min(l, 16)` known amplitudes (at their mean bin position, `m * model.k_q23`) along
/// `slope_q23_per_bin` (`envelope::envelope_slope_log2_per_bin_fixed`, at most 0). Integer only.
/// Returns the extended Q23 amplitudes (index 0 unused, `1..=l` copied) and the new count; with
/// `enabled` false, or no room above `l`, the count stays `model.l`.
// [@ANCHOR: extrapolate_amplitudes_from_envelope_fixed]
pub(crate) fn extrapolate_amplitudes_from_envelope_fixed(
    model: &ModelFixed,
    slope_q23_per_bin: i64,
    enabled: bool,
) -> ([i64; MAX_AMP_SB + 1], usize) {
    let l = model.l;
    let mut a_ext = [0i64; MAX_AMP_SB + 1];
    a_ext[1..=l].copy_from_slice(&model.a[1..=l]);
    let l2 = if enabled { (2 * l).min(MAX_AMP_SB) } else { l };
    if l2 <= l || l == 0 {
        return (a_ext, l);
    }
    let k = l.min(16);
    let (mut sx, mut sy) = (0i128, 0i128);
    for m in (l - k + 1)..=l {
        sx += m as i128 * model.k_q23 as i128;
        sy += log2_q23(model.a[m].max(1)) as i128;
    }
    let x0 = div_round_i128(sx, k as i128);
    let y0 = div_round_i128(sy, k as i128);
    let slope = slope_q23_per_bin.min(0) as i128;
    #[allow(clippy::needless_range_loop)]
    for m in (l + 1)..=l2 {
        let dx_q23 = m as i128 * model.k_q23 as i128 - x0 as i128;
        let y = y0 + rshift_round_i128(slope * dx_q23, FRAC_BITS);
        a_ext[m] = exp2_q23(y);
    }
    (a_ext, l2)
}

fn parzen_window_sb_q23() -> &'static [i64; SAMPLES_PER_FRAME_SB] {
    &super::tables::SB_PARZEN_Q23
}

/// Fixed-point sibling of `SpectralBridgeState` -- genuinely integer
/// end to end (no `f32` until the final `i16` PCM conversion, same
/// "integer core, float boundary" pattern `DecoderFixed` establishes
/// elsewhere in this crate). No `rng`/unvoiced excitation-phase branch
/// here at all, matching `synthesize_subframe_sb`'s own (post-review)
/// shape -- extrapolation only ever runs voiced.
pub(crate) struct SpectralBridgeStateFixed {
    pub(crate) enabled: bool,
    sn_: [i64; SAMPLES_PER_FRAME_SB],
    ex_phase_q32: u32,
    /// Phase source for the unvoiced high band (`synthesize_subframe_sb_from_envelope_fixed`),
    /// independent of the base synthesis's generator.
    rng: u32,
    ifft_re: [i64; FFT_ENC_SB],
    ifft_im: [i64; FFT_ENC_SB],
}

impl Default for SpectralBridgeStateFixed {
    fn default() -> Self {
        SpectralBridgeStateFixed {
            enabled: true,
            sn_: [0; SAMPLES_PER_FRAME_SB],
            ex_phase_q32: 0,
            rng: 0x5B_1D6E,
            ifft_re: [0; FFT_ENC_SB],
            ifft_im: [0; FFT_ENC_SB],
        }
    }
}

impl SpectralBridgeStateFixed {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Fixed-point `synthesize_subframe_sb`. `k_q23 = model.k_q23` (`synth_k_q23(wo)`,
    /// computed once in `ModelFixed::new`) is the *same* bin-index scaling `synthesize_subframe_fixed`
    /// already uses (built from the original `FFT_ENC`=512, not
    /// `FFT_ENC_SB` -- see this module's own doc comment on why the bin
    /// formula stays anchored to the original constant), just no longer
    /// clamped to `FFT_ENC/2`.
    // [@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_fixed]
    pub(crate) fn synthesize_subframe_sb_fixed(&mut self, model: &ModelFixed) -> [i16; N_SAMP_SB] {
        let (a_ext, l2) = extrapolate_amplitudes_fixed(model, self.enabled);
        self.synthesize_extended_fixed(model, &a_ext, l2)
    }

    /// Fixed-point `SpectralBridgeState::synthesize_subframe_sb_from_envelope`: the band above 4 kHz
    /// follows `slope_q23_per_bin` (`envelope::envelope_slope_log2_per_bin_fixed` of this
    /// sub-frame's LPC coefficients) from the top harmonics' mean level, for voiced and unvoiced
    /// sub-frames (random phases for unvoiced). `DecoderFixed::decode_16k_fixed` uses this one.
    /// Integer only. With `enabled` false it equals `synthesize_subframe_sb_fixed`.
    // [@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_from_envelope_fixed]
    pub(crate) fn synthesize_subframe_sb_from_envelope_fixed(
        &mut self,
        model: &ModelFixed,
        slope_q23_per_bin: i64,
    ) -> [i16; N_SAMP_SB] {
        let (a_ext, l2) =
            extrapolate_amplitudes_from_envelope_fixed(model, slope_q23_per_bin, self.enabled);
        self.synthesize_extended_fixed(model, &a_ext, l2)
    }

    /// The synthesis both fixed entry points share: decoded harmonics as `phi * a`, added ones with
    /// the voiced excitation phase (voiced) or a random phase from `rng` (unvoiced), 1024-point
    /// inverse transform, Parzen window, overlap-add.
    // [@ANCHOR: SpectralBridgeStateFixed::synthesize_extended_fixed]
    fn synthesize_extended_fixed(
        &mut self,
        model: &ModelFixed,
        a_ext: &[i64; MAX_AMP_SB + 1],
        l2: usize,
    ) -> [i16; N_SAMP_SB] {
        let increment = phase_increment_q32(model.wo);
        self.ex_phase_q32 = self.ex_phase_q32.wrapping_add(increment);
        let phi0_q32 = self.ex_phase_q32;

        let k_q23 = model.k_q23;
        let mut spectrum = SparseInverse::<FFT_ENC_SB>::new(&mut self.ifft_re, &mut self.ifft_im);
        #[allow(clippy::needless_range_loop)]
        for m in 1..=l2 {
            let raw = m as i64 * k_q23;
            let b = (((raw + (1i64 << 22)) >> 23) as usize).min(FFT_ENC_SB / 2 - 1);
            let bin = if m <= model.l {
                model.phi[m].mul(ComplexQ23 {
                    re: model.a[m],
                    im: 0,
                })
            } else if model.voiced {
                let angle_m_q32 = ((phi0_q32 as u64 * m as u64) & 0xFFFF_FFFF) as u32;
                sin_cos_q23(angle_m_q32).mul(ComplexQ23 {
                    re: a_ext[m],
                    im: 0,
                })
            } else {
                // Unvoiced (only the envelope entry point extends these): noise phases.
                sin_cos_q23(super::synthesis::next_rand_fixed(&mut self.rng)).mul(ComplexQ23 {
                    re: a_ext[m],
                    im: 0,
                })
            };
            spectrum.put(b, bin);
        }
        spectrum.run::<N_SAMP_SB>();

        overlap_add_subframe::<FFT_ENC_SB, N_SAMP_SB, SAMPLES_PER_FRAME_SB>(
            &mut self.sn_,
            &self.ifft_re,
            parzen_window_sb_q23(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::LPC_ORD;
    use super::*;

    /// Set from the measured worst cases with margin (2026-11-07: 0.0248 dB/kHz over 400 decoded
    /// envelopes; 0.0001 dB over four synthetic models).
    const SLOPE_TOLERANCE_DB_PER_KHZ: f64 = 0.1;
    const EXTRAPOLATION_TOLERANCE_DB: f32 = 0.01;
    use crate::codec2_3200::fixed_point::f32_to_q_exact_round;

    fn synthetic_model(wo: f32, voiced: bool, tilt_db_per_khz: f32) -> Model {
        let mut model = Model::new(wo, voiced);
        let freq_hz = |m: usize| m as f32 * wo * SAMPLE_RATE as f32 / std::f32::consts::TAU;
        for m in 1..=model.l {
            let db = -tilt_db_per_khz * (freq_hz(m) / 1000.0);
            model.a[m] = 1000.0 * 10f32.powf(db / 20.0);
            model.phi[m] = Complex32::new(1.0, 0.0);
        }
        model
    }

    /// Fixed-point sibling of `synthetic_model` -- same tilt formula,
    /// same construction, so `extrapolate_amplitudes_fixed`'s own
    /// output can be cross-checked directly against
    /// `extrapolate_amplitudes`'s float output on matching input.
    fn synthetic_model_fixed(wo: f32, voiced: bool, tilt_db_per_khz: f32) -> ModelFixed {
        let wo_q23 = f32_to_q_exact_round(wo, FRAC_BITS);
        let mut model = ModelFixed::new(wo_q23, voiced);
        let freq_hz = |m: usize| m as f32 * wo * SAMPLE_RATE as f32 / std::f32::consts::TAU;
        for m in 1..=model.l {
            let db = -tilt_db_per_khz * (freq_hz(m) / 1000.0);
            let amp = 1000.0 * 10f32.powf(db / 20.0);
            model.a[m] = f32_to_q_exact_round(amp, FRAC_BITS);
            model.phi[m] = ComplexQ23 {
                re: 1i64 << FRAC_BITS,
                im: 0,
            };
        }
        model
    }

    /// Cross-validates `extrapolate_amplitudes_fixed` against the float
    /// `extrapolate_amplitudes` on matching input -- same discipline
    /// `codec2_1600::lsp_quantiser`'s own fixed-vs-float cross-check
    /// uses (measure the real max error, then set the tolerance with
    /// real margin, never guessed).
    #[test]
    // Tests [@ANCHOR: extrapolate_amplitudes]
    // Tests [@ANCHOR: extrapolate_amplitudes_fixed]
    // Tests [@ANCHOR: freq_hz_fixed]
    // Tests [@ANCHOR: spectral_bridge_div_round_i128]
    fn extrapolate_amplitudes_fixed_matches_the_float_version_within_tolerance() {
        let wo = 180.0f32.to_radians().max(super::super::W0_MIN);
        let model_f = synthetic_model(wo, true, 9.0);
        let model_x = synthetic_model_fixed(wo, true, 9.0);
        assert_eq!(
            model_f.l, model_x.l,
            "float/fixed Model::new disagree on l for this wo"
        );

        let (a_ext_f, l2_f) = extrapolate_amplitudes(&model_f, true);
        let (a_ext_x, l2_x) = extrapolate_amplitudes_fixed(&model_x, true);
        assert_eq!(l2_f, l2_x, "float/fixed extrapolation disagree on l2");

        let mut max_rel_err = 0.0f32;
        #[allow(clippy::needless_range_loop)]
        for m in (model_f.l + 1)..=l2_f {
            let got = a_ext_x[m] as f32 / (1i64 << FRAC_BITS) as f32;
            let want = a_ext_f[m];
            max_rel_err = max_rel_err.max((got - want).abs() / want.max(1e-6));
        }
        println!("extrapolate_amplitudes_fixed vs float: max_rel_err={max_rel_err}");
        assert!(
            max_rel_err < 1e-4,
            "extrapolate_amplitudes_fixed diverged from the float version by {max_rel_err} (relative, measured 6.6e-7 when this test was written)"
        );
    }

    #[test]
    fn extrapolated_amplitudes_continue_a_declining_trend_without_exceeding_the_known_top() {
        let model = synthetic_model(super::super::W0_MIN, true, 12.0);
        let (a_ext, l2) = extrapolate_amplitudes(&model, true);
        assert!(
            l2 > model.l,
            "expected extrapolation to add harmonics, l={}, l2={l2}",
            model.l
        );
        assert_eq!(l2, (2 * model.l).min(MAX_AMP_SB));
        // A real declining trend should extrapolate to a value no
        // louder than the last known harmonic, and never negative/NaN.
        let last_known = model.a[model.l];
        #[allow(clippy::needless_range_loop)]
        for m in (model.l + 1)..=l2 {
            assert!(a_ext[m] >= 0.0, "negative extrapolated amplitude at m={m}");
            assert!(
                a_ext[m].is_finite(),
                "non-finite extrapolated amplitude at m={m}"
            );
            assert!(
                a_ext[m] <= last_known * 1.01,
                "extrapolated amplitude at m={m} ({}) exceeds the last known harmonic ({last_known}) -- a declining trend should never get louder",
                a_ext[m]
            );
        }
    }

    #[test]
    fn extrapolation_never_extends_past_max_amp_sb() {
        // Wo=W0_MIN gives l=MAX_AMP already -- 2*l would overflow
        // MAX_AMP_SB's own array bound if not clamped.
        let model = synthetic_model(super::super::W0_MIN, true, 6.0);
        let (_a_ext, l2) = extrapolate_amplitudes(&model, true);
        assert!(l2 <= MAX_AMP_SB, "l2={l2} exceeds MAX_AMP_SB={MAX_AMP_SB}");
    }

    /// Fixed-point sibling of `extrapolation_never_extends_past_max_amp_sb`
    /// -- the same overflow edge (`l=MAX_AMP` so `2*l=MAX_AMP_SB` exactly)
    /// exercised through `extrapolate_amplitudes_fixed`. If the
    /// `.min(MAX_AMP_SB)` clamp there were ever dropped, a release build
    /// would index `a_ext[MAX_AMP_SB+1]` out of bounds.
    #[test]
    fn extrapolation_fixed_never_extends_past_max_amp_sb() {
        // `ModelFixed::new`'s own Q23-quantized `wo` can land `l` one
        // harmonic below the float path's exact `l=MAX_AMP` at
        // `Wo=W0_MIN` (a known, already-documented rounding-boundary
        // discrepancy -- see `envelope.rs`'s own cross-check test), so
        // this sets `l` directly rather than relying on `Wo` alone to
        // reach the real overflow edge: at `l=MAX_AMP`, `2*l=MAX_AMP_SB`
        // exactly, the boundary the `.min(MAX_AMP_SB)` clamp exists for.
        let wo_q23 = f32_to_q_exact_round(super::super::W0_MIN, FRAC_BITS);
        let mut model = ModelFixed::new(wo_q23, true);
        model.l = MAX_AMP;
        let (_a_ext, l2) = extrapolate_amplitudes_fixed(&model, true);
        assert_eq!(
            l2, MAX_AMP_SB,
            "expected extrapolation to reach exactly MAX_AMP_SB at l=MAX_AMP"
        );
        assert!(l2 <= MAX_AMP_SB, "l2={l2} exceeds MAX_AMP_SB={MAX_AMP_SB}");
    }

    #[test]
    fn a_flat_spectrum_extrapolates_flat_not_rising() {
        // Zero tilt: the fitted trend should come out ~flat (beta~0),
        // not accidentally rising from floating-point noise in the
        // least-squares fit.
        let model = synthetic_model(150.0f32.to_radians().max(super::super::W0_MIN), true, 0.0);
        let (a_ext, l2) = extrapolate_amplitudes(&model, true);
        let last_known = model.a[model.l];
        #[allow(clippy::needless_range_loop)]
        for m in (model.l + 1)..=l2 {
            assert!(
                a_ext[m] <= last_known * 1.05,
                "flat input extrapolated to a rising trend at m={m}: {} vs known {last_known}",
                a_ext[m]
            );
        }
    }

    #[test]
    fn disabled_spectral_bridge_leaves_l2_at_the_original_harmonic_count() {
        let model = synthetic_model(200.0f32.to_radians().max(super::super::W0_MIN), true, 6.0);
        let (_a_ext, l2) = extrapolate_amplitudes(&model, false);
        assert_eq!(
            l2, model.l,
            "disabled Spectral Bridge should not add any harmonics"
        );
    }

    #[test]
    fn unvoiced_subframes_never_extrapolate_even_when_enabled() {
        let model = synthetic_model(200.0f32.to_radians().max(super::super::W0_MIN), false, 6.0);
        let (_a_ext, l2) = extrapolate_amplitudes(&model, true);
        assert_eq!(
            l2, model.l,
            "unvoiced sub-frames must not extrapolate, even when enabled=true"
        );
    }

    #[test]
    // Tests [@ANCHOR: SpectralBridgeState::synthesize_subframe_sb]
    // Tests [@ANCHOR: make_synthesis_window_sb]
    fn synthesize_subframe_sb_produces_finite_reasonably_scaled_audio_enabled_and_disabled() {
        for enabled in [true, false] {
            let mut sb = SpectralBridgeState::new();
            sb.enabled = enabled;
            let mut sumsq = 0.0f64;
            let mut n_samples = 0u64;
            let mut max_abs = 0i32;
            for frame_idx in 0..40 {
                let wo = super::super::W0_MIN + 0.001 * (frame_idx as f32 * 0.3).sin().abs();
                let model = synthetic_model(wo, frame_idx % 3 != 0, 9.0);
                let out = sb.synthesize_subframe_sb(&model);
                for &s in &out {
                    sumsq += (s as f64) * (s as f64);
                    max_abs = max_abs.max(s.abs() as i32);
                    n_samples += 1;
                }
            }
            let rms = (sumsq / n_samples as f64).sqrt();
            assert!(
                rms.is_finite() && rms >= 0.0,
                "enabled={enabled}: non-finite/negative RMS={rms}"
            );
            assert!(
                max_abs < 32768,
                "enabled={enabled}: clipped at the i16 boundary"
            );
        }
    }

    #[test]
    // Tests [@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_fixed]
    fn synthesize_subframe_sb_fixed_produces_finite_reasonably_scaled_audio_enabled_and_disabled() {
        for enabled in [true, false] {
            let mut sb = SpectralBridgeStateFixed::new();
            sb.enabled = enabled;
            let mut sumsq = 0.0f64;
            let mut n_samples = 0u64;
            let mut max_abs = 0i32;
            for frame_idx in 0..40 {
                let wo = super::super::W0_MIN + 0.001 * (frame_idx as f32 * 0.3).sin().abs();
                let model = synthetic_model_fixed(wo, frame_idx % 3 != 0, 9.0);
                let out = sb.synthesize_subframe_sb_fixed(&model);
                for &s in &out {
                    sumsq += (s as f64) * (s as f64);
                    max_abs = max_abs.max(s.abs() as i32);
                    n_samples += 1;
                }
            }
            let rms = (sumsq / n_samples as f64).sqrt();
            assert!(
                rms.is_finite() && rms >= 0.0,
                "enabled={enabled}: non-finite/negative RMS={rms}"
            );
            assert!(
                max_abs < 32768,
                "enabled={enabled}: clipped at the i16 boundary"
            );
        }
    }

    /// Known-answer check of the OLS fit's rounding division: ties round away from zero
    /// (`7/2 -> 4`, `-7/2 -> -4`), non-ties round to the nearest integer in both signs, exact
    /// quotients are untouched, and zero divides to zero. A plain truncating `n / d` would give
    /// 3, -3, 3, -3 for the first four cases below, so any loss of the half-divisor bias fails.
    #[test]
    // Tests [@ANCHOR: spectral_bridge_div_round_i128]
    fn div_round_i128_rounds_to_nearest_with_ties_away_from_zero() {
        assert_eq!(div_round_i128(7, 2), 4);
        assert_eq!(div_round_i128(-7, 2), -4);
        assert_eq!(div_round_i128(10, 3), 3); // 3.33 -> 3
        assert_eq!(div_round_i128(11, 3), 4); // 3.67 -> 4
        assert_eq!(div_round_i128(-10, 3), -3);
        assert_eq!(div_round_i128(-11, 3), -4);
        assert_eq!(div_round_i128(12, 4), 3);
        assert_eq!(div_round_i128(-12, 4), -3);
        assert_eq!(div_round_i128(0, 5), 0);
        // Values far beyond i64 intermediate range still divide exactly (the helper is i128).
        assert_eq!(div_round_i128((1i128 << 100) + 1, 1i128 << 40), 1i64 << 60);
    }

    #[test]
    fn harmonics_up_to_l_are_bit_identical_to_the_base_decode_when_extrapolation_is_disabled() {
        // With extrapolation disabled, the only content placed into the
        // bigger spectrum is exactly model.phi[m]*model.a[m] for
        // m=1..=l -- the same values the base 8kHz decode already
        // computed. This doesn't check the full 16kHz waveform (no
        // real reference exists to compare against), but it does
        // directly check the one real invariant this module's own
        // design promises: it must not alter the base harmonics at all.
        let model = synthetic_model(180.0f32.to_radians().max(super::super::W0_MIN), true, 9.0);
        let (a_ext, l2) = extrapolate_amplitudes(&model, false);
        assert_eq!(l2, model.l);
        #[allow(clippy::needless_range_loop)]
        for m in 1..=model.l {
            assert_eq!(
                a_ext[m], model.a[m],
                "harmonic {m} was altered when extrapolation is disabled"
            );
        }
    }

    /// An LPC spectrum whose envelope `1/|A|` falls `tilt_db_per_khz` per kHz, 257 bins (0..4 kHz).
    fn tilted_lpc_spectrum(tilt_db_per_khz: f32) -> Vec<Complex32> {
        let hz_per_bin = SAMPLE_RATE as f32 / FFT_ENC as f32;
        (0..=FFT_ENC / 2)
            .map(|i| {
                let mag_a = 10f32.powf(tilt_db_per_khz * (i as f32 * hz_per_bin / 1000.0) / 20.0);
                Complex32::new(mag_a, 0.0)
            })
            .collect()
    }

    // Tests [@ANCHOR: envelope_slope_ln_per_hz]
    #[test]
    fn envelope_slope_is_the_envelopes_tilt_and_never_rises() {
        let slope = envelope_slope_ln_per_hz(&tilted_lpc_spectrum(6.0));
        let want = -6.0 * std::f32::consts::LN_10 / 20.0 / 1000.0;
        assert!((slope - want).abs() < 1e-6, "slope {slope}, want {want}");
        assert!(envelope_slope_ln_per_hz(&tilted_lpc_spectrum(0.0)).abs() < 1e-12);
        // An envelope that rises toward 4 kHz is continued flat.
        assert_eq!(envelope_slope_ln_per_hz(&tilted_lpc_spectrum(-6.0)), 0.0);
        // Too short to fit: flat.
        assert_eq!(
            envelope_slope_ln_per_hz(&tilted_lpc_spectrum(6.0)[..100]),
            0.0
        );
    }

    // Tests [@ANCHOR: extrapolate_amplitudes_from_envelope]
    #[test]
    fn envelope_extrapolation_continues_voiced_and_unvoiced_frames_from_the_top_harmonics() {
        let slope = envelope_slope_ln_per_hz(&tilted_lpc_spectrum(6.0));
        for voiced in [true, false] {
            let model = synthetic_model(0.1, voiced, 6.0);
            let l = model.l;
            let (a, l2) = extrapolate_amplitudes_from_envelope(&model, slope, true);
            assert_eq!(l2, (2 * l).min(MAX_AMP_SB), "voiced {voiced}");
            assert_eq!(&a[1..=l], &model.a[1..=l]);
            // Falling 6 dB per kHz from the top of the known band, never above it.
            for m in (l + 1)..l2 {
                assert!(a[m + 1] < a[m] && a[m] <= model.a[l], "m {m}");
            }
            let hz = |m: usize| m as f32 * model.wo * SAMPLE_RATE as f32 / std::f32::consts::TAU;
            let db_per_khz = 20.0 * (a[l2] / a[l + 1]).log10() / ((hz(l2) - hz(l + 1)) / 1000.0);
            assert!((db_per_khz + 6.0).abs() < 0.05, "{db_per_khz} dB/kHz");
            let (_, l_off) = extrapolate_amplitudes_from_envelope(&model, slope, false);
            assert_eq!(l_off, l);
        }
    }

    // Tests [@ANCHOR: SpectralBridgeState::synthesize_subframe_sb_from_envelope]
    // Tests [@ANCHOR: SpectralBridgeState::synthesize_extended]
    #[test]
    fn the_envelope_bridge_puts_energy_above_4_khz_for_unvoiced_frames_and_matches_when_disabled() {
        let aw = tilted_lpc_spectrum(3.0);
        // Power from 4.5 to 8 kHz of the whole 16 kHz output, by FFT (above the edge, where the
        // known top harmonics' window leakage sits).
        let high_band_energy = |pcm: &[i16]| -> f64 {
            let mut buf: Vec<Complex32> =
                pcm.iter().map(|&s| Complex32::new(s as f32, 0.0)).collect();
            rustfft::FftPlanner::new()
                .plan_fft_forward(buf.len())
                .process(&mut buf);
            let n = buf.len();
            buf[n * 9 / 32..n / 2]
                .iter()
                .map(|c| c.norm_sqr() as f64)
                .sum()
        };
        let mut old = SpectralBridgeState::new();
        let mut new = SpectralBridgeState::new();
        let (mut out_old, mut out_new) = (Vec::new(), Vec::new());
        for _ in 0..20 {
            let mut model = synthetic_model(0.12, false, 3.0);
            for m in 1..=model.l {
                model.phi[m] = Complex32::from_polar(1.0, m as f32 * 1.7);
            }
            out_old.extend(old.synthesize_subframe_sb(&model));
            let out = new.synthesize_subframe_sb_from_envelope(&model, &aw);
            assert!(out.iter().all(|s| s.unsigned_abs() < 32767));
            out_new.extend(out);
        }
        let (e_old, e_new) = (high_band_energy(&out_old), high_band_energy(&out_new));
        // The old bridge leaves an unvoiced frame's high band empty but for leakage.
        assert!(
            e_new > 10.0 * e_old.max(1.0),
            "unvoiced high band: old {e_old}, new {e_new}"
        );

        // Disabled: both entry points give the same, harmonics 1..=l only.
        let mut a = SpectralBridgeState::new();
        let mut b = SpectralBridgeState::new();
        a.enabled = false;
        b.enabled = false;
        for voiced in [true, false] {
            let model = synthetic_model(0.1, voiced, 6.0);
            assert_eq!(
                a.synthesize_subframe_sb(&model),
                b.synthesize_subframe_sb_from_envelope(&model, &aw)
            );
        }
    }

    /// LPC sets as the decoder sees them: decoded from pseudo-random line spectral pair indexes,
    /// float and Q23 from the same indexes.
    fn decoded_lpc_sets(count: usize) -> Vec<([f32; LPC_ORD + 1], [i64; LPC_ORD + 1])> {
        use super::super::{lpc, quantise};
        let mut seed = 0x1234_5678u32;
        (0..count)
            .map(|_| {
                let idx: [u32; LPC_ORD] = core::array::from_fn(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed % 16
                });
                let f = lpc::lsp_to_lpc(&quantise::decode_lsps_delta_scalar(&idx));
                let q = lpc::lsp_to_lpc_fixed(&quantise::decode_lsps_delta_scalar_fixed(&idx));
                (f, q)
            })
            .collect()
    }

    /// Float slope (natural log per Hz) as the fixed format (Q23 log2 per 15.625 Hz bin).
    fn slope_as_q23_per_bin(ln_per_hz: f32) -> f64 {
        ln_per_hz as f64 * (SAMPLE_RATE as f64 / FFT_ENC as f64) / std::f64::consts::LN_2
            * (1u64 << FRAC_BITS) as f64
    }

    // Tests [@ANCHOR: envelope_slope_log2_per_bin_fixed]
    #[test]
    fn the_fixed_envelope_slope_matches_the_float_one() {
        use super::super::envelope::{
            compute_harmonic_amplitudes, envelope_slope_log2_per_bin_fixed,
        };
        use super::super::fixed_fft::FftScratch;
        let mut scratch = FftScratch::new();
        let mut worst = 0.0f64;
        let mut negative = 0;
        for (ak_f, ak_q) in decoded_lpc_sets(400) {
            let mut model = Model::new(0.1, true);
            let aw = compute_harmonic_amplitudes(&ak_f, 1000.0, &mut model);
            let want = slope_as_q23_per_bin(envelope_slope_ln_per_hz(&aw));
            let got = envelope_slope_log2_per_bin_fixed(&ak_q, &mut scratch) as f64;
            assert!(got <= 0.0);
            negative += (got < 0.0) as usize;
            // Error in dB per kHz: Q23 log2 per bin -> 20 log10(2) dB per 15.625 Hz.
            let db_per_khz = |q: f64| q / (1u64 << FRAC_BITS) as f64 * 6.0206 * 64.0;
            worst = worst.max((db_per_khz(got) - db_per_khz(want)).abs());
        }
        println!("fixed vs float envelope slope: worst difference {worst:.4} dB/kHz, {negative} of 400 falling");
        assert!(negative > 100, "the sets must exercise falling envelopes");
        assert!(worst < SLOPE_TOLERANCE_DB_PER_KHZ, "worst {worst} dB/kHz");
    }

    // Tests [@ANCHOR: extrapolate_amplitudes_from_envelope_fixed]
    #[test]
    fn the_fixed_envelope_extrapolation_matches_the_float_one() {
        let mut worst_db = 0.0f32;
        for (wo, tilt, voiced) in [
            (0.06, 3.0, true),
            (0.1, 9.0, false),
            (0.2, 6.0, true),
            (0.3, 0.0, false),
        ] {
            let fm = synthetic_model(wo, voiced, tilt);
            let qm = synthetic_model_fixed(wo, voiced, tilt);
            let slope_ln = -tilt * std::f32::consts::LN_10 / 20.0 / 1000.0;
            let (af, lf) = extrapolate_amplitudes_from_envelope(&fm, slope_ln, true);
            let (aq, lq) = extrapolate_amplitudes_from_envelope_fixed(
                &qm,
                slope_as_q23_per_bin(slope_ln).round() as i64,
                true,
            );
            assert_eq!(lf, lq, "wo {wo}");
            assert_eq!(lq, (2 * qm.l).min(MAX_AMP_SB));
            for m in (qm.l + 1)..=lq {
                let q = aq[m] as f32 / (1u64 << FRAC_BITS) as f32;
                worst_db = worst_db.max((20.0 * (q / af[m]).log10()).abs());
            }
            let (_, off) = extrapolate_amplitudes_from_envelope_fixed(&qm, -1000, false);
            assert_eq!(off, qm.l);
        }
        println!("fixed vs float envelope extrapolation: worst {worst_db:.4} dB");
        assert!(worst_db < EXTRAPOLATION_TOLERANCE_DB, "worst {worst_db} dB");
    }

    // Tests [@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_from_envelope_fixed]
    // Tests [@ANCHOR: SpectralBridgeStateFixed::synthesize_extended_fixed]
    #[test]
    fn the_fixed_envelope_bridge_extends_unvoiced_frames_and_matches_the_old_one_when_disabled() {
        let high = |pcm: &[i16]| -> f64 {
            let mut buf: Vec<Complex32> =
                pcm.iter().map(|&s| Complex32::new(s as f32, 0.0)).collect();
            rustfft::FftPlanner::new()
                .plan_fft_forward(buf.len())
                .process(&mut buf);
            let n = buf.len();
            buf[n * 9 / 32..n / 2]
                .iter()
                .map(|c| c.norm_sqr() as f64)
                .sum()
        };
        let slope =
            slope_as_q23_per_bin(-3.0 * std::f32::consts::LN_10 / 20.0 / 1000.0).round() as i64;
        let (mut old, mut new) = (
            SpectralBridgeStateFixed::new(),
            SpectralBridgeStateFixed::new(),
        );
        let (mut out_old, mut out_new) = (Vec::new(), Vec::new());
        for _ in 0..20 {
            let mut model = synthetic_model_fixed(0.12, false, 3.0);
            for m in 1..=model.l {
                model.phi[m] = sin_cos_q23((m as u32).wrapping_mul(0x4567_89AB));
            }
            out_old.extend(old.synthesize_subframe_sb_fixed(&model));
            out_new.extend(new.synthesize_subframe_sb_from_envelope_fixed(&model, slope));
        }
        let (e_old, e_new) = (high(&out_old), high(&out_new));
        assert!(e_new > 10.0 * e_old.max(1.0), "old {e_old}, new {e_new}");
        let (mut a, mut b) = (
            SpectralBridgeStateFixed::new(),
            SpectralBridgeStateFixed::new(),
        );
        a.enabled = false;
        b.enabled = false;
        for voiced in [true, false] {
            let model = synthetic_model_fixed(0.1, voiced, 6.0);
            assert_eq!(
                a.synthesize_subframe_sb_fixed(&model),
                b.synthesize_subframe_sb_from_envelope_fixed(&model, slope)
            );
        }
    }
}
