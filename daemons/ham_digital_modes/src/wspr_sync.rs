// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]

//! WSPR sync search + tone detection: finds a real WSPR transmission's
//! actual frequency and time offset inside a raw audio capture, then
//! extracts the per-symbol soft evidence `wspr_decode.rs`'s sequential
//! decoder needs -- step 3 of `docs/proposals/
//! WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s build order. Uses `weakmon`'s
//! `wspr.py` (MIT-licensed) purely as an algorithmic reference -- same
//! "read the protocol facts and reimplement" discipline `wspr.rs`'s own
//! encoder and `wspr_decode.rs`'s own decoder doc comments already
//! establish, not a port. Unlike FT8 (`ft8.rs`), which vendors a real
//! permissively-licensed C decoder (`ft8_lib`) via FFI instead of a
//! from-scratch DSP reimplementation, no equivalent exists for WSPR: the
//! real reference decoder (`wsprd`, part of WSJT-X) is GPL, and the
//! MIT-licensed reference actually consulted here (`weakmon`) is pure
//! Python, not a C library that could be FFI'd in the same way.
//!
//! ## Scope, deliberately narrower than a real reference decoder
//!
//! Per advisor review before this was written: **single-strongest-signal
//! only, no decode-and-subtract loop, and frequency drift fixed at
//! zero.** `weakmon`'s own `process()` runs two passes (decode, then
//! subtract every decode found and decode again) specifically to pull
//! multiple *simultaneous, overlapping* WSPR transmissions out of one
//! crowded band -- a real optimization layered on top of a working
//! single-signal decoder, not part of one. This module finds and decodes
//! the single strongest candidate only. It also does not search
//! frequency drift (a transmitter's own frequency changing slowly across
//! the 162-symbol transmission) at all -- `find_sync()`'s search only
//! ever evaluates zero drift. Both are real, known simplifications for a
//! first working version, not oversights; the noise-ladder fixtures this
//! module gets verified against (`wspr.rs`'s own `add_awgn()`) are
//! single-signal and drift-free, so this is exactly what step 6's real
//! verification can actually exercise.
//!
//! **A real finding from building this module's own tests, worth
//! carrying forward past this file**: `find_sync()` is a coherent
//! correlator, integrating a per-symbol statistic across all 162
//! symbols -- exactly the mechanism that lets real WSPR pull a
//! signal up from well below what a naive per-sample SNR would
//! suggest is detectable. That cuts both ways. While developing this
//! module's own noise-rejection test, subtracting a real transmission
//! from its own noisy version (`noisy - clean`) was tried first as "no
//! real signal present" -- even though the per-sample residual measured
//! a near-zero simple correlation coefficient (~0.009) with the
//! original signal, this search still detected it as a near-perfect
//! sync match (confirmed to be the real signal's own faint residual,
//! not a search artifact, by checking the identical noise against an
//! unrelated arbitrary pattern, which scored far lower). Two real
//! consequences: (1) "subtract the signal and call the remainder noise"
//! is not a valid way to construct a negative test case for this kind
//! of detector -- see `find_sync_and_decode_correctly_reject_
//! genuinely_independent_noise()`'s own doc comment for the corrected
//! methodology; and (2) this module's single-strongest-signal scope
//! (no decode-and-subtract loop, see above) means a real *adjacent*
//! WSPR transmission sharing the same band is a live, unexamined risk
//! for false sync -- not tested here, and not something the noise-only
//! rejection test above can stand in for. Worth real attention when
//! step 6 moves to actual band recordings, which routinely have more
//! than one WSPR signal in view.
//!
//! ## The core sync statistic (the real algorithmic fact taken from `weakmon`)
//!
//! WSPR's 4-FSK tone at real transmission position `i` encodes
//! `tone = 2*data_bit + sync_bit` (see `wspr.rs`'s own `wspr_encode_
//! symbols()`: `symbols[i] = 2 * interleaved[i] + SYNC_VECTOR[i]`), so
//! tones {0,2} are the "sync bit = 0" tones and {1,3} are "sync bit = 1"
//! tones, regardless of the (unknown, to be decoded) data bit. Since
//! `SYNC_VECTOR` is a fixed, publicly known 162-symbol pattern every
//! WSPR receiver already knows, the per-symbol statistic
//! `tt[i] = max(tone1,tone3) - max(tone0,tone2)` correlates strongly
//! with `SYNC_VECTOR` (remapped to ±1) if and only if the analysis
//! window is aligned to the real transmission's actual frequency and
//! timing -- a wrong frequency or timing hypothesis sees uncorrelated
//! tone energy instead. This is the one fact this module's entire search
//! (`find_sync()`) is built on: sweep frequency/timing hypotheses,
//! score each by this correlation, keep the best.
//!
//! Once the winning (frequency, timing) hypothesis is found, the known
//! sync bit at each position also tells the decoder which of the 4 tone
//! bins is "data bit = 0" evidence (`tone[sync_bit]`) and which is
//! "data bit = 1" evidence (`tone[2+sync_bit]`) -- `extract_symbol_
//! evidence()` below.
//!
//! ## Mapping FFT magnitudes onto the decoder's bipolar-Gaussian model
//!
//! `wspr_decode.rs`'s `fano_bit_metric()` assumes a bipolar Gaussian
//! channel (`r ≈ ±amplitude + N(0, noise_stddev²)`). Real FFT tone
//! magnitudes are not that -- they're Rayleigh/Rician-distributed and
//! strictly positive, and naively differencing two of them is not
//! automatically zero-mean-Gaussian or correctly scaled. Per advisor
//! review, this module picks the lower-new-code option deliberately (a
//! second option existed: a Bayes-combined two-probability metric
//! entry point mirroring Karn's own approach more closely, which
//! `weakmon` uses -- not built here): feed `r[i] = v1[i] - v0[i]`
//! directly into the existing bipolar metric, with `amplitude` and
//! `noise_stddev` estimated empirically from the same 162-symbol run
//! (`evidence_to_symbol_values()` below) rather than assumed. This is a
//! heuristic calibration, not a rigorously derived one -- what actually
//! validates or falsifies it is real measurement against the already-
//! recorded `wsprd` ground truth (`WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s
//! own noise-ladder table), not a closed-form proof, consistent with
//! this codebase's own "measure, don't guess" discipline.
//!
//! One specific open question within that heuristic, real and not yet
//! resolved either way: `evidence_to_symbol_values()` computes
//! `noise_stddev = sqrt((var(winner) + var(loser)) / 2)` (an average of
//! the two per-symbol tone-magnitude variances). Since `r = v1 - v0` is
//! a difference of two roughly-independent quantities, the variance of
//! a *difference* of independent random variables is normally the *sum*
//! of their variances, not the average -- `sqrt(var(winner) +
//! var(loser))`, without the `/2`, is the more standard derivation and
//! was flagged as such during design review. The `/2` form was kept
//! deliberately for this first version anyway (it's what was reviewed
//! and is already tested end-to-end), but this is a real fork in the
//! calibration, not a rounding detail -- step 6's real-capture
//! verification should sweep both forms against the noise-ladder ground
//! truth rather than assume the current one is correct.

use crate::wspr::{SYNC_VECTOR, WSPR_NUM_SYMBOLS, WSPR_SYMBOL_RATE_HZ};
use crate::wspr_decode::{
    deinterleave_symbol_values, sequential_decode_with_confidence_gate, ConfidenceGateError,
};
use rustfft::num_complex::Complex64;
use rustfft::FftPlanner;

/// Audio samples spanning one WSPR symbol at `sample_rate` -- also
/// exactly the STFT block length this module uses, since WSPR's own
/// tone spacing equals its symbol rate (`WSPR_SYMBOL_RATE_HZ`), so an
/// `spsym`-point FFT's own bin spacing lands exactly on WSPR's tone
/// spacing with no extra zero-padding needed.
fn samples_per_symbol(sample_rate: u32) -> usize {
    (sample_rate as f64 / WSPR_SYMBOL_RATE_HZ).round() as usize
}

/// The number of samples a full 162-symbol WSPR transmission occupies at
/// `sample_rate` -- exactly the width `find_sync()`'s own search window
/// needs starting at any candidate `start_sample`. Callers building
/// `max_start_sample` for `find_sync()`/`sync_search_and_decode()`/
/// `sync_search_and_decode_message()` MUST subtract this from their
/// buffer length, not pass the buffer length (or length - 1) directly:
/// `find_sync()` already probes every offset up to `max_start_sample`,
/// so an overlong `max_start_sample` doesn't find anything more (no
/// start offset beyond `buffer_len - required_window_samples()` can
/// ever contain a full window, so `spectrum_matrix()` just returns
/// `None` for those and the extra iterations are pure wasted search
/// cost) -- for a real ~125s accumulation buffer at 12kHz this
/// difference is roughly an order of magnitude more STFT work than the
/// search actually needs, which is what made the first real end-to-end
/// pipeline test (`wspr_decode_fires_at_a_real_utc_slot_boundary`) blow
/// through its own timeout: the search was still grinding through
/// offsets that could never contain a real window.
pub fn required_window_samples(sample_rate: u32) -> usize {
    WSPR_NUM_SYMBOLS * samples_per_symbol(sample_rate)
}

/// Decimates 48kHz mono audio down to WSPR's own native 12kHz sample
/// rate (a plain 4-sample box average -- crude but real low-pass
/// filtering, not a proper FIR decimator; upgrade to one if real
/// captures ever show aliasing artifacts this coarse a filter can't
/// reject). Real callers (`hams_local_relay`'s own digital-mode
/// pipeline, which runs its mic input at 48kHz) should decimate before
/// calling `find_sync()`/`sync_search_and_decode_message()`, not run
/// the search at 48kHz directly: `find_sync()`'s own search cost scales
/// with the FFT block size (`samples_per_symbol()`, itself proportional
/// to `sample_rate`), so running at 48kHz would cost ~4.6x an identical
/// search at 12kHz for the same time resolution -- the per-candidate
/// time step is `samples_per_symbol()/8`, which scales with the sample
/// rate the same way the FFT does, so decimating first is a pure win,
/// not a resolution/cost tradeoff. Just as importantly, this module's
/// own tests -- including `MIN_SYNC_SCORE`'s own measured calibration --
/// all run at 12kHz; decimating first keeps production audio on the
/// same, tested code path rather than an unmeasured 48kHz one. Any
/// trailing samples that don't fill a complete group of 4 are dropped,
/// not padded.
pub fn decimate_4x_box_average(samples: &[i16]) -> Vec<i16> {
    samples
        .chunks_exact(4)
        .map(|chunk| {
            let sum: i32 = chunk.iter().map(|&s| s as i32).sum();
            (sum / 4) as i16
        })
        .collect()
}

/// Computes `|FFT(block)|` for each of the 162 symbol-length blocks
/// starting at `start_sample`, down-converting each block by
/// `sub_bin_hz_offset` first (multiplying by a complex exponential
/// before the FFT) so a candidate frequency that falls between two FFT
/// bins can still be evaluated at exact-bin precision after the shift --
/// the same purpose `weakmon`'s own `freq_shift()`-before-FFT step
/// serves, reimplemented directly against `rustfft` rather than ported.
/// Returns only the bins in `[bin_lo, bin_hi_inclusive]` (the search
/// band plus 3 extra bins so the top candidate's 4th tone is still
/// covered), computed once and shared across every candidate base bin
/// in that range -- `weakmon`'s own `coarse()` does the same sharing
/// (one `rfft` per block covers every bin at once), which is what makes
/// scanning many candidate frequencies computationally tractable instead
/// of re-running a whole 162-block STFT per candidate bin.
/// `fft` must already be planned for exactly `samples_per_symbol(sample_
/// rate)` points -- callers that invoke this many times for the same
/// `sample_rate` (`find_sync()`'s own search loop) plan it once and pass
/// it down, rather than this function re-planning (and `FftPlanner`
/// re-searching for a fast factorization of the block size) on every
/// single call, the one real per-candidate cost in the whole search.
fn spectrum_matrix(
    samples: &[i16],
    sample_rate: u32,
    fft: &dyn rustfft::Fft<f64>,
    bin_lo: usize,
    bin_hi_inclusive: usize,
    sub_bin_hz_offset: f64,
    start_sample: usize,
) -> Option<Vec<Vec<f64>>> {
    let spsym = samples_per_symbol(sample_rate);
    if start_sample + WSPR_NUM_SYMBOLS * spsym > samples.len() {
        return None;
    }
    let two_pi = std::f64::consts::TAU;
    let nbins = bin_hi_inclusive - bin_lo + 1;

    let mut mat = vec![vec![0.0f64; nbins]; WSPR_NUM_SYMBOLS];
    for (sym, row) in mat.iter_mut().enumerate() {
        let block_start = start_sample + sym * spsym;
        let mut buf: Vec<Complex64> = (0..spsym)
            .map(|n| {
                let sample = samples[block_start + n] as f64;
                let phase = -two_pi * sub_bin_hz_offset * (n as f64) / (sample_rate as f64);
                Complex64::new(sample * phase.cos(), sample * phase.sin())
            })
            .collect();
        fft.process(&mut buf);
        for (j, bin) in (bin_lo..=bin_hi_inclusive).enumerate() {
            row[j] = buf[bin].norm();
        }
    }
    Some(mat)
}

fn tone_magnitudes_from_matrix(
    mat: &[Vec<f64>],
    bin_lo: usize,
    base_bin: usize,
) -> [[f64; 4]; WSPR_NUM_SYMBOLS] {
    let mut out = [[0.0f64; 4]; WSPR_NUM_SYMBOLS];
    let j0 = base_bin - bin_lo;
    for sym in 0..WSPR_NUM_SYMBOLS {
        for (tone, slot) in out[sym].iter_mut().enumerate() {
            *slot = mat[sym][j0 + tone];
        }
    }
    out
}

/// `SYNC_VECTOR` remapped to ±1, the target this module's own sync
/// statistic gets correlated against.
fn sync_pattern() -> [f64; WSPR_NUM_SYMBOLS] {
    let mut p = [0.0f64; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        p[i] = if SYNC_VECTOR[i] == 1 { 1.0 } else { -1.0 };
    }
    p
}

/// `tt[i] = max(tone1,tone3) - max(tone0,tone2)`, raw (not yet scaled or
/// normalized) -- see this module's own doc comment for why this
/// specific statistic correlates with `SYNC_VECTOR` exactly when the
/// analysis window is correctly aligned. See `cosine_score()` below for
/// how this gets turned into a comparable, bounded score across
/// candidates.
fn sync_statistic(tone_mags: &[[f64; 4]; WSPR_NUM_SYMBOLS]) -> [f64; WSPR_NUM_SYMBOLS] {
    let mut tt = [0.0f64; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let syncs0 = tone_mags[i][0].max(tone_mags[i][2]);
        let syncs1 = tone_mags[i][1].max(tone_mags[i][3]);
        tt[i] = syncs1 - syncs0;
    }
    tt
}

/// Cosine similarity between `tt` and `pattern` (`sum(tt[i]*pattern[i])
/// / (||tt|| * ||pattern||)`), bounded in `[-1, 1]` and equal to 1.0
/// only when `tt` is an exact positive scalar multiple of `pattern` --
/// **not** `weakmon`'s own `numpy.correlate(tt/mean(abs(tt)), pattern)`
/// approach (dividing by the mean absolute value instead of the L2
/// norm), which was tried here first and found to be a real, serious
/// bug (not just a cosmetic scaling difference): since `pattern[i]` is
/// always exactly ±1, dividing `tt` by `mean(|tt|)` makes the resulting
/// score EXACTLY 162 (the maximum) whenever every single `tt[i]` merely
/// has the same SIGN as `pattern[i]`, regardless of how large or small
/// each `tt[i]`'s own magnitude is relative to the others -- the
/// statistic collapses to one bit of information ("did every sign
/// agree") and throws away the magnitude evidence that should separate
/// a real transmission from a coincidental sign match. Confirmed
/// directly: real, non-clipped, near-white Gaussian noise (no WSPR
/// transmission anywhere in the tested audio) reliably found a
/// `find_sync()` candidate scoring the exact theoretical maximum under
/// the mean-abs version, which `sequential_decode_with_confidence_
/// gate()` then accepted as a confident, wrong decode -- exactly the
/// silent-wrong-answer failure class this codebase's own fail-fast
/// convention exists to prevent, now caught before this module was ever
/// committed rather than found later against real captures. L2
/// normalization instead gives a proper, smoothly graded correlation
/// coefficient that a real signal's own strong magnitude separation
/// (not just sign agreement) pushes toward 1.0, and pure noise does not
/// -- see `find_sync_score_on_pure_noise_is_far_below_a_real_signals_
/// score` (renamed from this bug's own original diagnostic test) for
/// the real, measured separation this version achieves.
fn cosine_score(tt: &[f64; WSPR_NUM_SYMBOLS], pattern: &[f64; WSPR_NUM_SYMBOLS]) -> f64 {
    let dot: f64 = tt.iter().zip(pattern.iter()).map(|(a, b)| a * b).sum();
    let tt_norm: f64 = tt.iter().map(|v| v * v).sum::<f64>().sqrt();
    let pattern_norm: f64 = (WSPR_NUM_SYMBOLS as f64).sqrt(); // pattern[i] = ±1 always.
    if tt_norm > 0.0 {
        dot / (tt_norm * pattern_norm)
    } else {
        0.0
    }
}

/// `find_sync()`'s own default minimum acceptable `sync_score` --
/// candidates scoring below this are treated as "no real signal found"
/// (`None`), not returned as a false best-effort guess. Measured (not
/// guessed), the same "measure, don't guess" discipline as `wspr_
/// decode.rs`'s own `MIN_ACCEPTABLE_METRIC`: genuinely independent
/// noise (no real WSPR transmission anywhere in the tested audio, 5
/// different seeds) scored at most ~0.29 under `cosine_score()`; a real
/// transmission, swept down to -30dB (near `wsprd`'s own real
/// sensitivity floor, per `WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s own
/// noise-ladder table), scored at least ~0.65. `0.4` sits between those
/// two measured ENDPOINTS, not picked by curve-fitting a single test
/// case -- but the region between them is NOT itself characterized: 0.29
/// is the max of 5 seeds at one noise level, not a swept maximum, and
/// -30dB is exactly where `wsprd` itself goes seed-dependent per the
/// noise-ladder table, so there is no measurement of how the score
/// actually behaves for real signals weaker than -30dB or for noise
/// levels/realizations beyond those 5 seeds. Treat `0.4` as a real,
/// reasoned starting point sitting in a real gap, not as a corridor
/// that's been swept end to end. Like `MIN_ACCEPTABLE_METRIC`, this is a
/// caller-supplied argument on `find_sync()`/`sync_search_and_decode()`
/// (not a hardcoded internal constant) -- pass `f64::NEG_INFINITY` for
/// the raw, ungated best-effort candidate instead (deliberately
/// bypassing this gate, not a bug when a caller does it on purpose; see
/// this module's own test of the same name suffix for an example). The
/// right false-accept/false-reject tradeoff otherwise depends on the
/// caller (a public-database WSPR spot wants a stricter bar than a
/// local display). A finding worth keeping in view for step 6's real-
/// capture verification, not treated as closed: this default's margin
/// was measured against SYNTHETIC noise and synthetic-plus-noise
/// signals only, not real receiver noise or real off-frequency
/// interference (including a second, real, adjacent WSPR transmission
/// -- see this module's own "Scope" doc comment above), which may have
/// different spectral character than simulated white Gaussian noise.
pub const MIN_SYNC_SCORE: f64 = 0.4;

/// The winning (frequency, timing) hypothesis `find_sync()` found, plus
/// its own raw correlation score (higher is a stronger sync match --
/// not itself a calibrated probability or SNR estimate, just a ranking
/// statistic across candidates within one search). Guaranteed `>=` the
/// `min_sync_score` the caller passed to `find_sync()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncResult {
    pub base_hz: f64,
    pub start_sample: usize,
    pub sync_score: f64,
}

/// Searches candidate base frequencies in `[freq_lo_hz, freq_hi_hz]` (in
/// `WSPR_SYMBOL_RATE_HZ`-wide bins, each also probed at `sub_bin_steps`
/// fractional offsets within the bin) and candidate start sample offsets
/// in `[0, max_start_sample]` (stepped every `samples_per_symbol()/8`),
/// scoring each by `cosine_score()`'s own correlation of `sync_
/// statistic()` against `sync_pattern()`. Returns the single
/// best-scoring candidate, or `None` if `samples` is too short for even
/// one full 162-symbol window OR the best candidate found still scored
/// below `min_sync_score` (pass `MIN_SYNC_SCORE` for the documented,
/// measured default -- see its own doc comment for what it protects
/// against and why it's a caller-supplied argument, not a hardcoded
/// gate). Single-strongest-signal only, zero drift only -- see this
/// module's own doc comment on scope. `sync_score` on a returned result
/// is a real, bounded (`[-1, 1]`) correlation coefficient, not a
/// raw/unbounded statistic -- see `cosine_score()`'s own doc comment for
/// why that distinction is load-bearing, not cosmetic.
pub fn find_sync(
    samples: &[i16],
    sample_rate: u32,
    freq_lo_hz: f64,
    freq_hi_hz: f64,
    max_start_sample: usize,
    min_sync_score: f64,
) -> Option<SyncResult> {
    let bin_hz = WSPR_SYMBOL_RATE_HZ;
    if freq_lo_hz < 0.0 || freq_hi_hz <= freq_lo_hz {
        return None;
    }
    let bin_lo = (freq_lo_hz / bin_hz).floor() as usize;
    // +3 so the top candidate base bin's 4th tone (base_bin+3) is still
    // inside the shared spectrum_matrix() window.
    let bin_hi = (freq_hi_hz / bin_hz).ceil() as usize + 3;
    let spsym = samples_per_symbol(sample_rate);
    let sub_bin_steps = 8usize;
    // Time-offset search resolution: a residual timing error up to half
    // this step leaks adjacent-symbol energy into each STFT block
    // (samples from the wrong symbol land inside the "current" block),
    // degrading tone-magnitude accuracy -- this is not a free parameter
    // with no accuracy consequence, it's a real coarse/fine tradeoff
    // step 6's calibration against the noise-ladder ground truth still
    // needs to characterize, not just this step's own clean/moderate-
    // noise tests (which only prove the search converges at all).
    let time_step = (spsym / 8).max(1);
    let pattern = sync_pattern();
    // Planned once and shared across every (sub_bin_offset, start_
    // sample) candidate in the search below, not re-planned per call --
    // see spectrum_matrix()'s own doc comment on why that matters here.
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(spsym);

    let mut best: Option<SyncResult> = None;
    let mut start_sample = 0usize;
    loop {
        for sub in 0..sub_bin_steps {
            let sub_bin_hz_offset = (sub as f64) * bin_hz / (sub_bin_steps as f64);
            let Some(mat) = spectrum_matrix(
                samples,
                sample_rate,
                fft.as_ref(),
                bin_lo,
                bin_hi,
                sub_bin_hz_offset,
                start_sample,
            ) else {
                continue;
            };
            for base_bin in bin_lo..=(bin_hi - 3) {
                let tone_mags = tone_magnitudes_from_matrix(&mat, bin_lo, base_bin);
                let tt = sync_statistic(&tone_mags);
                let score = cosine_score(&tt, &pattern);
                if best.as_ref().is_none_or(|b| score > b.sync_score) {
                    best = Some(SyncResult {
                        base_hz: (base_bin as f64) * bin_hz + sub_bin_hz_offset,
                        start_sample,
                        sync_score: score,
                    });
                }
            }
        }
        if start_sample >= max_start_sample {
            break;
        }
        start_sample = (start_sample + time_step).min(max_start_sample);
    }
    best.filter(|b| b.sync_score >= min_sync_score)
}

/// For a known (or `find_sync()`-recovered) `base_hz`/`start_sample`,
/// extracts per-real-transmitted-symbol `[v0, v1]` tone-magnitude
/// evidence: `v0` is the magnitude of whichever tone bin the known
/// `SYNC_VECTOR` bit at that position says is "data bit = 0" evidence
/// (tone index = the sync bit), `v1` is "data bit = 1" evidence (tone
/// index = 2 + sync bit) -- see this module's own doc comment for why
/// this specific pair is the right one to read off. Returned in real
/// transmission order (matching `wspr.rs`'s own `SYNC_VECTOR`/
/// `interleave_permutation()` target-array indexing), NOT yet
/// de-interleaved into the decoder's encoder-order convention -- see
/// `evidence_to_symbol_values()` below for that step.
pub fn extract_symbol_evidence(
    samples: &[i16],
    sample_rate: u32,
    base_hz: f64,
    start_sample: usize,
) -> Option<[[f64; 2]; WSPR_NUM_SYMBOLS]> {
    let tone_mags = extract_all_four_tone_magnitudes(samples, sample_rate, base_hz, start_sample)?;

    let mut evidence = [[0.0f64; 2]; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let sync_bit = SYNC_VECTOR[i] as usize;
        evidence[i][0] = tone_mags[i][sync_bit];
        evidence[i][1] = tone_mags[i][2 + sync_bit];
    }
    Some(evidence)
}

/// Shared FFT step behind `extract_symbol_evidence()`, factored out so
/// diagnostics can also read the two tone bins `extract_symbol_evidence()`
/// itself discards (indices `1 - sync_bit`/`3 - sync_bit` at each symbol --
/// per `wspr.rs`'s own `symbols[i] = 2 * interleaved[i] + SYNC_VECTOR[i]`
/// encoding, these two bins can NEVER carry the real transmitted tone given
/// the already-known sync bit, so they are a clean, always-pure-noise
/// reference at each symbol position, uncontaminated by the winner/loser
/// max/min selection bias `evidence_to_symbol_values()`'s own calibration
/// has -- see `diagnostic_clean_noise_reference_from_impossible_tones`.
fn extract_all_four_tone_magnitudes(
    samples: &[i16],
    sample_rate: u32,
    base_hz: f64,
    start_sample: usize,
) -> Option<[[f64; 4]; WSPR_NUM_SYMBOLS]> {
    let bin_hz = WSPR_SYMBOL_RATE_HZ;
    let base_bin = (base_hz / bin_hz).round().max(0.0) as usize;
    let sub_bin_hz_offset = base_hz - (base_bin as f64) * bin_hz;
    let spsym = samples_per_symbol(sample_rate);
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(spsym);
    let mat = spectrum_matrix(
        samples,
        sample_rate,
        fft.as_ref(),
        base_bin,
        base_bin + 3,
        sub_bin_hz_offset,
        start_sample,
    )?;
    Some(tone_magnitudes_from_matrix(&mat, base_bin, base_bin))
}

/// The other half of `extract_all_four_tone_magnitudes()`'s output --
/// per symbol position, the two FFT bins that `extract_symbol_evidence()`
/// itself never reads (indices `1 - sync_bit`/`3 - sync_bit`, per
/// `wspr.rs`'s own `symbols[i] = 2 * interleaved[i] + SYNC_VECTOR[i]`
/// encoding). These can NEVER carry the real transmitted tone given the
/// already-known sync bit, so they are a clean, always-pure-noise
/// reference at each symbol position, free of `evidence_to_symbol_
/// values()`'s own winner/loser order-statistics bias -- exactly the
/// "impossible tone" reference the Per-Symbol Channel Model Redesign
/// (`docs/proposals/WSPR_DECODE_IMPLEMENTATION_PLAN.md`) uses to build a
/// local per-symbol noise_stddev estimate. Returned in real transmission
/// order, same convention as `extract_symbol_evidence()`.
pub fn extract_impossible_tone_evidence(
    samples: &[i16],
    sample_rate: u32,
    base_hz: f64,
    start_sample: usize,
) -> Option<[[f64; 2]; WSPR_NUM_SYMBOLS]> {
    let tone_mags = extract_all_four_tone_magnitudes(samples, sample_rate, base_hz, start_sample)?;
    let mut evidence = [[0.0f64; 2]; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let sync_bit = SYNC_VECTOR[i] as usize;
        evidence[i][0] = tone_mags[i][1 - sync_bit];
        evidence[i][1] = tone_mags[i][3 - sync_bit];
    }
    Some(evidence)
}

/// Sliding-window half-width (in symbols) used to compute a LOCAL
/// per-symbol `amplitude`/`noise_stddev` estimate instead of one global
/// pair -- see `docs/proposals/WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s own
/// "Per-Symbol Channel Model Redesign" section for the full evidence
/// this responds to (a decisive diagnostic showed the decoder's search
/// algorithm and branch-metric formula are sound; the single global
/// scalar pair is the wrong *shape* to describe real per-symbol-varying
/// channel reliability at low SNR).
///
/// K=40 (half-width 20, i.e. each symbol's estimate pools itself plus 20
/// neighbors on each side) was chosen empirically, not from the doc's
/// own initial K=9-16 starting-point guess -- `diagnostic_windowed_
/// winner_loser_estimator_sweep_at_the_failure_boundary` swept half-
/// widths 8/20/40 (paired with the summed-variance noise formula
/// `evidence_to_symbol_values_windowed()`'s own doc comment describes);
/// half-widths 20 and 40 tied for the best measured -30dB result (6/15
/// correct), half-width 8 did meaningfully worse (5/15). 20 was kept
/// over the tied 40 as the more locally-responsive of the two, on the
/// (untested) theory that a narrower window should track genuine local
/// channel variation more closely -- a real, honest tie-break judgment
/// call, not a result the sweep itself distinguished.
const CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH: usize = 20;

/// Converts `extract_symbol_evidence()`'s real, positive tone-magnitude
/// pairs into the bipolar-Gaussian `(symbol_values, amplitude,
/// noise_stddev)` `wspr_decode.rs`'s `sequential_decode()` family
/// expects -- see this module's own doc comment ("Mapping FFT
/// magnitudes...") for the calibration this implements and why it's a
/// heuristic, not a derivation. `symbol_values[i] = v1[i] - v0[i]` (still
/// in real transmission order, matching `extract_symbol_evidence()`'s
/// own convention -- callers pass all three returned arrays through
/// `deinterleave_symbol_values()` before calling the decoder, same as
/// `wspr_decode.rs`'s own tests do, so `amplitude[p]`/`noise_stddev[p]`
/// line up positionally with `channel_bit_values[p]` after that step).
///
/// `amplitude`/`noise_stddev` are LOCAL, per-symbol estimates
/// (Per-Symbol Channel Model Redesign), not the single global pair this
/// function used to return before 2026-09-01 -- each symbol position `i`
/// pools a sliding window of `2*CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH+1`
/// neighboring symbols (clamped at the transmission's own start/end,
/// never wrapping): windowed winner/loser separation (`amplitude[i] =
/// mean(winner) - mean(loser)`, same calculation this function always
/// used, just localized instead of pooled over all 162 symbols) and
/// windowed **summed** (not averaged) per-bin variance (`noise_stddev[i]
/// = sqrt(winvar + losevar)`, NOT the old `/2` -- see below for why).
///
/// **Real, measured history behind this specific formula (2026-09-01)**:
/// the design doc's own PREFERRED first candidate -- a windowed clean-
/// noise reference from `extract_impossible_tone_evidence()`'s
/// guaranteed-pure-noise bins, order-statistics-bias-free by
/// construction -- was built, measured, and found to give ZERO correct
/// decodes at the real failing SNR range (-30dB and below) across every
/// window half-width tested (2, 4, 8, 20, 40, 81, 161 -- the full range
/// from maximally local to fully global), including a regression at
/// -30dB relative to the pre-redesign baseline (1/5 correct -> 0/5). A
/// direct scale-ratio check (`diagnostic_clean_noise_reference_from_
/// impossible_tones`) ruled out a units/scale bug as the explanation
/// (clean-vs-raw ratio stayed within ~0.97-1.14 near the boundary, not a
/// 2x-or-more mismatch) -- the clean reference is a genuinely worse
/// noise estimator at these SNRs, not merely mis-scaled. This is now
/// confirmed at every window size, not just the doc's own already-
/// recorded global-scope finding ("performs worse, not better").
/// `extract_impossible_tone_evidence()`/`evidence_to_symbol_values_
/// windowed_clean_reference()` remain in this module purely as the
/// tested, reproducible record of that finding (`diagnostic_window_
/// half_width_sweep_at_the_failure_boundary`), not as production code.
///
/// The design doc's OTHER candidate -- windowed winner/loser, the same
/// mechanism the pre-redesign global scalar always used, just localized
/// -- was tried next (`diagnostic_windowed_winner_loser_estimator_sweep_
/// at_the_failure_boundary`) and gave a real, if partial, win: summing
/// the per-bin variances (`sqrt(winvar+losevar)`, matching the direction
/// the pre-redesign `diagnostic_noise_stddev_scale_factor_sweep_at_the_
/// failure_boundary` already found helpful) at `window_half_width=20`
/// roughly TRIPLED the -30dB correct-decode rate relative to the
/// pre-redesign baseline (1/5 -> up to 6/15 in the 15-seed sweep, i.e.
/// ~20% -> ~40%), with no combination tested regressing below the
/// baseline anywhere. The amplitude order-statistics debias term the
/// design doc also suggested trying alongside this (`amplitude - k *
/// noise_stddev_raw`) was swept too and made results WORSE at every
/// point tested, so it is deliberately NOT applied here. This does NOT
/// close the deeper part of the gap -- -31.5dB and below stayed at 0/15
/// across every combination swept, matching the pre-redesign baseline's
/// own 0/5 there. See `docs/proposals/WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s
/// own "Built, and partially measured to work" section for the full
/// writeup and the real, still-open remainder of the gap.
pub fn evidence_to_symbol_values(
    evidence: &[[f64; 2]; WSPR_NUM_SYMBOLS],
) -> (
    [f64; WSPR_NUM_SYMBOLS],
    [f64; WSPR_NUM_SYMBOLS],
    [f64; WSPR_NUM_SYMBOLS],
) {
    evidence_to_symbol_values_windowed(evidence, CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH)
}

/// `evidence_to_symbol_values()`'s actual implementation, parameterized
/// on the window half-width so diagnostics/tests can sweep it directly
/// rather than only through the fixed public constant -- see `docs/
/// proposals/WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s own K-tuning writeup
/// for the sweep this was measured against.
fn evidence_to_symbol_values_windowed(
    evidence: &[[f64; 2]; WSPR_NUM_SYMBOLS],
    window_half_width: usize,
) -> (
    [f64; WSPR_NUM_SYMBOLS],
    [f64; WSPR_NUM_SYMBOLS],
    [f64; WSPR_NUM_SYMBOLS],
) {
    let mut symbol_values = [0.0f64; WSPR_NUM_SYMBOLS];
    let mut winners = [0.0f64; WSPR_NUM_SYMBOLS];
    let mut losers = [0.0f64; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let v0 = evidence[i][0];
        let v1 = evidence[i][1];
        symbol_values[i] = v1 - v0;
        winners[i] = v0.max(v1);
        losers[i] = v0.min(v1);
    }

    // Floors applied per-window below (not just once at the end): a
    // pathological all-zero or all-equal window (e.g. silence) must not
    // hand the decoder a zero/negative amplitude or a zero noise_stddev
    // for ANY symbol position -- both would make fano_bit_metric()'s
    // Gaussian shape (and its log2()) undefined or degenerate rather
    // than just producing a low-confidence (correctly rejected) decode.
    let mut amplitude = [0.0f64; WSPR_NUM_SYMBOLS];
    let mut noise_stddev = [0.0f64; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let lo = i.saturating_sub(window_half_width);
        let hi = (i + window_half_width).min(WSPR_NUM_SYMBOLS - 1);
        let window_n = (hi - lo + 1) as f64;

        let win_winmean = winners[lo..=hi].iter().sum::<f64>() / window_n;
        let win_losemean = losers[lo..=hi].iter().sum::<f64>() / window_n;
        amplitude[i] = (win_winmean - win_losemean).max(1e-9);

        let winvar = winners[lo..=hi]
            .iter()
            .map(|x| (x - win_winmean) * (x - win_winmean))
            .sum::<f64>()
            / window_n;
        let losevar = losers[lo..=hi]
            .iter()
            .map(|x| (x - win_losemean) * (x - win_losemean))
            .sum::<f64>()
            / window_n;
        // Summed, not averaged (no `/2`) -- see this function's own
        // caller doc comment for the measured evidence behind this.
        noise_stddev[i] = (winvar + losevar).sqrt().max(1e-9);
    }

    (symbol_values, amplitude, noise_stddev)
}

/// The design doc's originally-preferred candidate estimator -- kept as
/// a diagnostic-only historical record (see `evidence_to_symbol_values()`'s
/// own doc comment for why it isn't production code): windowed local
/// noise_stddev from `extract_impossible_tone_evidence()`'s guaranteed-
/// pure-noise samples instead of winner/loser variance. `amplitude` here
/// still comes from the same windowed winner/loser calculation --  only
/// `noise_stddev`'s source differs from `evidence_to_symbol_values_
/// windowed()` above.
fn evidence_to_symbol_values_windowed_clean_reference(
    evidence: &[[f64; 2]; WSPR_NUM_SYMBOLS],
    impossible_tone_evidence: &[[f64; 2]; WSPR_NUM_SYMBOLS],
    window_half_width: usize,
) -> (
    [f64; WSPR_NUM_SYMBOLS],
    [f64; WSPR_NUM_SYMBOLS],
    [f64; WSPR_NUM_SYMBOLS],
) {
    let mut symbol_values = [0.0f64; WSPR_NUM_SYMBOLS];
    let mut winners = [0.0f64; WSPR_NUM_SYMBOLS];
    let mut losers = [0.0f64; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let v0 = evidence[i][0];
        let v1 = evidence[i][1];
        symbol_values[i] = v1 - v0;
        winners[i] = v0.max(v1);
        losers[i] = v0.min(v1);
    }

    let mut amplitude = [0.0f64; WSPR_NUM_SYMBOLS];
    let mut noise_stddev = [0.0f64; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let lo = i.saturating_sub(window_half_width);
        let hi = (i + window_half_width).min(WSPR_NUM_SYMBOLS - 1);
        let window_n = (hi - lo + 1) as f64;

        let win_winmean = winners[lo..=hi].iter().sum::<f64>() / window_n;
        let win_losemean = losers[lo..=hi].iter().sum::<f64>() / window_n;
        amplitude[i] = (win_winmean - win_losemean).max(1e-9);

        // A `Vec` here (not a fixed-size stack array) deliberately --
        // `window_half_width` is a runtime parameter (diagnostics sweep
        // it directly), so a stack array sized off the fixed public
        // constant would silently overflow for any larger sweep value.
        let mut clean_samples = Vec::with_capacity(2 * (hi - lo + 1));
        for pair in &impossible_tone_evidence[lo..=hi] {
            clean_samples.push(pair[0]);
            clean_samples.push(pair[1]);
        }
        let clean_n = clean_samples.len();
        let clean_mean = clean_samples.iter().sum::<f64>() / (clean_n as f64);
        let clean_var = clean_samples
            .iter()
            .map(|x| (x - clean_mean) * (x - clean_mean))
            .sum::<f64>()
            / (clean_n as f64);
        noise_stddev[i] = clean_var.sqrt().max(1e-9);
    }

    (symbol_values, amplitude, noise_stddev)
}

/// `evidence_to_symbol_values()` plus de-interleaving plus the
/// confidence-gated decoder -- the function real callers (a future
/// `digital_decoder.rs` integration included) should actually use to go
/// from raw per-symbol evidence to a decode attempt.
pub fn decode_from_symbol_evidence(
    evidence: &[[f64; 2]; WSPR_NUM_SYMBOLS],
    max_cycles: u64,
    min_acceptable_metric: f64,
) -> Result<u128, ConfidenceGateError> {
    let (symbol_values, amplitude, noise_stddev) = evidence_to_symbol_values(evidence);
    let channel_bit_values = deinterleave_symbol_values(&symbol_values);
    let channel_amplitude = deinterleave_symbol_values(&amplitude);
    let channel_noise_stddev = deinterleave_symbol_values(&noise_stddev);
    sequential_decode_with_confidence_gate(
        &channel_bit_values,
        &channel_amplitude,
        &channel_noise_stddev,
        max_cycles,
        min_acceptable_metric,
    )
}

/// End-to-end: search `samples` for the best (frequency, timing) sync
/// candidate in the given search ranges (rejecting any candidate below
/// `min_sync_score` -- pass `MIN_SYNC_SCORE` for the documented,
/// measured default), extract its symbol evidence, and attempt a
/// decode. Returns `None` if `samples` was too short for `find_sync()`
/// to evaluate even one candidate, OR if no candidate cleared `min_
/// sync_score` -- both real "no signal found" outcomes, not
/// distinguished from each other here. A real decode failure past that
/// point (gave up, or a low-confidence reject) still returns
/// `Some((base_hz, Err(...)))`, not `None` -- "no signal found in this
/// audio" and "found a signal but couldn't decode it" are different,
/// real outcomes a caller may want to distinguish.
///
/// The returned `base_hz` is `find_sync()`'s own detected audio
/// frequency of the signal within `[freq_lo_hz, freq_hi_hz]` -- surfaced
/// (not just consumed internally to compute symbol evidence) because
/// `AUTO_TUNE_AND_MODE_DETECTION.md`'s own "auto tune" needs exactly
/// this: the caller already knows what dial frequency the rig sat at
/// while these samples were captured, so `base_hz` minus WSPR's own
/// conventional passband-center reference (1500Hz, the frequency every
/// synthetic fixture in this file already encodes at) gives the real
/// tuning correction needed to center a detected signal. Returned even
/// on a decode failure (`Err`), since a caller may still want to know
/// where a low-confidence signal was found.
#[allow(clippy::too_many_arguments)] // Each parameter is an independent, real search/decode tuning knob -- see the doc comments on find_sync()/decode_from_symbol_evidence() for what each one means; bundling them into a struct would just move the same count, not reduce it.
pub fn sync_search_and_decode(
    samples: &[i16],
    sample_rate: u32,
    freq_lo_hz: f64,
    freq_hi_hz: f64,
    max_start_sample: usize,
    min_sync_score: f64,
    max_cycles: u64,
    min_acceptable_metric: f64,
) -> Option<(f64, Result<u128, ConfidenceGateError>)> {
    let sync = find_sync(
        samples,
        sample_rate,
        freq_lo_hz,
        freq_hi_hz,
        max_start_sample,
        min_sync_score,
    )?;
    let evidence = extract_symbol_evidence(samples, sample_rate, sync.base_hz, sync.start_sample)?;
    Some((
        sync.base_hz,
        decode_from_symbol_evidence(&evidence, max_cycles, min_acceptable_metric),
    ))
}

/// What `sync_search_and_decode_message()` returns for a failure --
/// `ConfidenceGateError`'s own two variants, plus a real, distinct third
/// outcome `ConfidenceGateError` alone can't represent: the decoder
/// reached a confident (metric-accepted) bit pattern, but that bit
/// pattern still doesn't unpack into a valid Type-1 message (`wspr.rs`'s
/// own `unpack_wspr_message()` returned `None`) -- a real "confident in
/// something that isn't a real encodable message" outcome, not the same
/// as either a low-confidence reject or a search give-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WsprMessageError {
    GaveUp { cycles: u64 },
    LowConfidence { bits: u128, metric: f64 },
    UnpackFailed { bits: u128 },
}

impl From<ConfidenceGateError> for WsprMessageError {
    fn from(e: ConfidenceGateError) -> Self {
        match e {
            ConfidenceGateError::GaveUp { cycles } => WsprMessageError::GaveUp { cycles },
            ConfidenceGateError::LowConfidence { bits, metric } => {
                WsprMessageError::LowConfidence { bits, metric }
            }
        }
    }
}

/// `sync_search_and_decode()` plus `wspr.rs`'s own `unpack_wspr_
/// message()` -- the real end-to-end entry point this build order's
/// step 4 exists for: raw audio in, a human-readable `(callsign, grid4,
/// power_dbm, base_hz)` out, or a real, distinguishable failure reason.
/// The function a future `digital_decoder.rs` integration (step 5)
/// should actually call. `base_hz` is `sync_search_and_decode()`'s own
/// detected audio frequency -- see that function's doc comment for why
/// it's surfaced (`AUTO_TUNE_AND_MODE_DETECTION.md`'s real tuning-
/// correction use).
#[allow(clippy::too_many_arguments)] // Same reasoning as sync_search_and_decode()'s own allow -- see its doc comment.
pub fn sync_search_and_decode_message(
    samples: &[i16],
    sample_rate: u32,
    freq_lo_hz: f64,
    freq_hi_hz: f64,
    max_start_sample: usize,
    min_sync_score: f64,
    max_cycles: u64,
    min_acceptable_metric: f64,
) -> Option<Result<(String, String, i32, f64), WsprMessageError>> {
    let (base_hz, result) = sync_search_and_decode(
        samples,
        sample_rate,
        freq_lo_hz,
        freq_hi_hz,
        max_start_sample,
        min_sync_score,
        max_cycles,
        min_acceptable_metric,
    )?;
    Some(match result {
        Ok(bits) => crate::wspr::unpack_wspr_message(bits)
            .map(|(callsign, grid, power_dbm)| (callsign, grid, power_dbm, base_hz))
            .ok_or(WsprMessageError::UnpackFailed { bits }),
        Err(e) => Err(e.into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wspr::{
        add_awgn, pack_call, pack_grid4_power, unpack_wspr_message, wspr_encode_audio,
        wspr_encode_symbols, wspr_modulate,
    };
    use crate::wspr_decode::WSPR_DECODABLE_INPUT_BITS;

    /// Independently reconstructs the first `WSPR_DECODABLE_INPUT_BITS`
    /// real info bits `wspr_encode_symbols()` transmits -- same
    /// construction as `wspr_decode.rs`'s own private test helper of the
    /// same name (deliberately duplicated, not shared, same reasoning as
    /// that module's own duplicated noise generator: keeping this
    /// module's own tests independently correct rather than trusting
    /// another module's test-only code to stay in sync). Built directly
    /// from `data`'s own MSB-first bit order (matching `convolutional_
    /// encode()`'s own bit-consumption order, i.e. the decoder's own
    /// trellis-depth order) rather than through `SYNC_VECTOR`/
    /// `interleave_permutation()` -- a much simpler, less error-prone
    /// path than re-deriving the same value by inverting the interleave.
    fn expected_decodable_bits(callsign: &str, grid4: &str, power_dbm: i32) -> u128 {
        let n = pack_call(callsign).unwrap();
        let m = pack_grid4_power(grid4, power_dbm).unwrap();
        let mut data = [0u8; 11];
        data[0] = ((n >> 20) & 0xFF) as u8;
        data[1] = ((n >> 12) & 0xFF) as u8;
        data[2] = ((n >> 4) & 0xFF) as u8;
        data[3] = (((n & 0x0F) << 4) + ((m >> 18) & 0x0F)) as u8;
        data[4] = ((m >> 10) & 0xFF) as u8;
        data[5] = ((m >> 2) & 0xFF) as u8;
        data[6] = ((m & 0x03) << 6) as u8;

        let mut bits: u128 = 0;
        for i in 0..WSPR_DECODABLE_INPUT_BITS {
            let byte = data[i / 8];
            let bit = (byte >> (7 - (i % 8))) & 1;
            if bit == 1 {
                bits |= 1u128 << i;
            }
        }
        bits
    }

    /// Isolates "did I get the tone-to-bit mapping right" from "did the
    /// search find the signal" (advisor-recommended first test, before
    /// any search loop): known frequency, known zero time offset, no
    /// noise. Confirms the sync statistic actually correlates with
    /// `SYNC_VECTOR`, and that the extracted (v0, v1) evidence recovers
    /// every real transmitted data bit exactly.
    #[test]
    fn sync_statistic_and_tone_evidence_are_correct_at_a_known_clean_alignment() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let bin_hz = WSPR_SYMBOL_RATE_HZ;
        let sample_rate = 12000u32;
        // On the bin grid exactly, so base_bin/sub_bin_hz_offset are
        // trivial (0) -- isolates the mapping itself from interpolation.
        let base_bin = (1500.0 / bin_hz).round() as usize;
        let base_hz = (base_bin as f64) * bin_hz;
        let audio = wspr_modulate(&symbols, base_hz, sample_rate);

        let evidence = extract_symbol_evidence(&audio, sample_rate, base_hz, 0)
            .expect("clean fixture must be long enough for a full window at offset 0");

        let spsym = samples_per_symbol(sample_rate);
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(spsym);
        let mat = spectrum_matrix(
            &audio,
            sample_rate,
            fft.as_ref(),
            base_bin,
            base_bin + 3,
            0.0,
            0,
        )
        .unwrap();
        let tone_mags = tone_magnitudes_from_matrix(&mat, base_bin, base_bin);
        let tt = sync_statistic(&tone_mags);
        let pattern = sync_pattern();
        let score = cosine_score(&tt, &pattern);
        // A real, strong positive correlation close to the maximum
        // possible (1.0) -- not just "greater than zero," which a lucky
        // coincidence could satisfy.
        assert!(score > 0.9, "sync statistic should correlate strongly with SYNC_VECTOR at a known-correct alignment, got score={score}");

        for i in 0..WSPR_NUM_SYMBOLS {
            let expected_data_bit = (symbols[i] >> 1) & 1;
            let v0 = evidence[i][0];
            let v1 = evidence[i][1];
            let recovered_bit = if v1 > v0 { 1u8 } else { 0u8 };
            assert_eq!(
                recovered_bit, expected_data_bit,
                "symbol {i}: v0={v0} v1={v1}, expected data bit {expected_data_bit}"
            );
        }
    }

    /// The same mapping, but the true frequency deliberately does NOT
    /// land on an exact FFT bin -- proves the sub-bin down-conversion
    /// step (not exercised by the bin-aligned test above) is itself
    /// correct, not just the on-grid case.
    #[test]
    fn tone_evidence_is_correct_at_a_frequency_between_two_fft_bins() {
        let symbols = wspr_encode_symbols("W1AW", "FN31", 37).unwrap();
        let sample_rate = 12000u32;
        let base_hz = 1500.37; // deliberately off the ~1.4648Hz bin grid.
        let audio = wspr_modulate(&symbols, base_hz, sample_rate);

        let evidence = extract_symbol_evidence(&audio, sample_rate, base_hz, 0).unwrap();
        for i in 0..WSPR_NUM_SYMBOLS {
            let expected_data_bit = (symbols[i] >> 1) & 1;
            let recovered_bit = if evidence[i][1] > evidence[i][0] {
                1u8
            } else {
                0u8
            };
            assert_eq!(
                recovered_bit, expected_data_bit,
                "symbol {i} at an off-grid frequency"
            );
        }
    }

    /// Per-Symbol Channel Model Redesign, step 4's own required test:
    /// proves the windowed local noise_stddev estimator actually DETECTS
    /// and localizes a deliberate noise burst injected only on symbols
    /// 50-110, rather than just reproducing the old global-average
    /// behavior with extra steps -- the real thing this redesign needs
    /// to prove, not just "it doesn't regress the existing passing
    /// cases." Tests the PRODUCTION estimator (`evidence_to_symbol_
    /// values_windowed()`, windowed winner/loser with summed variance --
    /// see that function's own caller doc comment for why this formula,
    /// not the clean-noise-reference one, is what shipped) directly on a
    /// synthetic `evidence` array (no real audio/FFT), so the noise
    /// injected is exact and fully controlled, isolating the windowing
    /// logic itself from any FFT-extraction noise. Burst region widened
    /// from an earlier 40-60 draft to 50-110 so it comfortably contains
    /// a full window at the real, empirically-chosen
    /// `CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH=20`.
    #[test]
    fn windowed_noise_estimate_detects_and_localizes_a_deliberate_noise_burst() {
        const BURST_START: usize = 50;
        const BURST_END: usize = 110; // inclusive
                                      // Both regions alternate two winner/loser gap widths symbol-to-
                                      // symbol, so each window has real, nonzero LOCAL variance for
                                      // the summed-variance formula to actually measure -- a constant
                                      // gap (even at a different level) would have zero local
                                      // variance and wouldn't exercise the estimator at all. Burst
                                      // gaps are ~33x wider than baseline gaps, a decisive, not
                                      // marginal, contrast.
        let mut evidence = [[0.0f64, 0.0f64]; WSPR_NUM_SYMBOLS];
        for (i, slot) in evidence.iter_mut().enumerate() {
            let in_burst = (BURST_START..=BURST_END).contains(&i);
            *slot = match (in_burst, i % 2 == 0) {
                (false, true) => [995.0, 1005.0],
                (false, false) => [998.0, 1002.0],
                (true, true) => [800.0, 1200.0],
                (true, false) => [900.0, 1100.0],
            };
        }

        let (_, _, noise_stddev) =
            evidence_to_symbol_values_windowed(&evidence, CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH);

        // Deep inside the burst (a window fully contained in [50,110]
        // given the real window half-width), the estimator should report
        // a noise_stddev far above the baseline level.
        let mid_burst = (BURST_START + BURST_END) / 2;
        assert!(
            mid_burst >= BURST_START + CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH
                && mid_burst + CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH <= BURST_END,
            "test setup: mid_burst's own window must be fully inside the burst region"
        );
        assert!(
            noise_stddev[mid_burst] > 30.0,
            "expected the windowed estimator to detect the local noise burst at symbol {mid_burst}, \
             got noise_stddev={} (baseline level is ~2.1)",
            noise_stddev[mid_burst]
        );

        // Well outside the burst (this symbol's own window doesn't
        // overlap [50,110] at all), noise_stddev should stay near the
        // small baseline level, not be dragged up by the burst elsewhere.
        let quiet_symbol = 10;
        assert!(
            quiet_symbol + CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH < BURST_START,
            "test setup: quiet_symbol's own window must not overlap the burst region at all"
        );
        assert!(
            noise_stddev[quiet_symbol] < 10.0,
            "expected the windowed estimator to stay near the baseline far from the burst, got \
             noise_stddev={} at symbol {quiet_symbol} (baseline level is ~2.1)",
            noise_stddev[quiet_symbol]
        );

        // The real point of this test: prove the WINDOWED estimate finds
        // something a GLOBAL (old-behavior) estimate washes out. A
        // window wide enough to cover the whole transmission reproduces
        // the old global-average behavior as a special case (see
        // `evidence_to_symbol_values()`'s own doc comment) -- confirm
        // its report at the same two positions shows far less contrast
        // than the windowed one, i.e. it genuinely blurs the localized
        // burst away into one shared number.
        let (_, _, global_noise_stddev) =
            evidence_to_symbol_values_windowed(&evidence, WSPR_NUM_SYMBOLS);
        let windowed_contrast = noise_stddev[mid_burst] / noise_stddev[quiet_symbol];
        let global_contrast = global_noise_stddev[mid_burst] / global_noise_stddev[quiet_symbol];
        assert!(
            windowed_contrast > global_contrast * 2.0,
            "windowed estimator should show far more contrast between the burst and quiet regions \
             than the old global-average behavior -- windowed_contrast={windowed_contrast:.2}, \
             global_contrast={global_contrast:.2} (global should be ~1.0, since it pools everything \
             into one shared value)"
        );
    }

    /// The actual search: a clean fixture at a frequency and start
    /// offset `find_sync()` is NOT told in advance (only a surrounding
    /// range), across a real multi-bin/multi-offset grid -- confirms
    /// the search itself (not just the mapping) finds the right answer.
    #[test]
    fn find_sync_recovers_the_real_frequency_and_start_offset() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let sample_rate = 12000u32;
        let true_hz = 1503.2;
        let true_start_symbols_padding = 3; // symbols of silence before the real transmission
        let spsym = samples_per_symbol(sample_rate);

        let tx = wspr_modulate(&symbols, true_hz, sample_rate);
        let mut audio = vec![0i16; true_start_symbols_padding * spsym];
        audio.extend_from_slice(&tx);
        audio.extend_from_slice(&[0i16; 4]); // trailing pad so a slightly-over-shot search window still fits.

        let sync = find_sync(
            &audio,
            sample_rate,
            1490.0,
            1510.0,
            5 * spsym,
            MIN_SYNC_SCORE,
        )
        .expect("a real, clean, in-band signal must be found");

        let bin_hz = WSPR_SYMBOL_RATE_HZ;
        assert!(
            (sync.base_hz - true_hz).abs() < bin_hz / 2.0,
            "recovered frequency {} too far from the real {true_hz}",
            sync.base_hz
        );
        let true_start_sample = true_start_symbols_padding * spsym;
        assert!(
            (sync.start_sample as i64 - true_start_sample as i64).unsigned_abs()
                <= (spsym / 8) as u64,
            "recovered start_sample {} too far from the real {true_start_sample}",
            sync.start_sample
        );
    }

    /// Advisor-flagged real gap, closed by measurement -- and the
    /// regression guard for two real bugs this test's own development
    /// caught before this module was ever committed.
    ///
    /// **Bug 1, in the original test methodology, not in `wspr_sync.rs`
    /// itself**: "pure noise" was first modeled as `noisy - clean`
    /// (subtracting a real transmission from its own `add_awgn()`
    /// output). That is NOT a valid "no real signal present" case: even
    /// though the per-sample residual is genuinely small and near-
    /// independent of `clean` in a simple correlation-coefficient sense,
    /// this module's own sync search is a coherent correlator
    /// integrating over all 162 symbols -- *exactly* the mechanism real
    /// WSPR decoding relies on to pull a real signal up from well below
    /// a naive noise floor. A residual this small can still carry a
    /// faint, genuinely coherent trace of the real signal it was
    /// subtracted from, and this search correctly (not erroneously)
    /// detects that trace -- confirmed directly: the identical noise
    /// buffer scored ~0.99 against the real `SYNC_VECTOR` pattern but
    /// only ~0.31 against an unrelated, arbitrary fixed ±1 pattern of
    /// the same length, ruling out generic search overfitting and
    /// pointing specifically at real leaked signal structure. Fixed
    /// here to noise with no mathematical relationship to any WSPR
    /// transmission at all (`add_awgn()` against a constant, non-signal
    /// reference, with that reference subtracted back out) -- genuinely
    /// independent across 5 different seeds, this scores at most ~0.29.
    ///
    /// **Bug 2, real, in `wspr_sync.rs` itself, found via the (flawed
    /// but still noise-like) case above before this fix**: the original
    /// scoring function normalized `tt` by `mean(|tt|)` (`weakmon`'s own
    /// approach) rather than its L2 norm. Since `pattern[i]` is always
    /// exactly ±1, that normalization makes the resulting score EXACTLY
    /// equal to its own theoretical maximum whenever every single
    /// `tt[i]` merely has the same SIGN as `pattern[i]` -- regardless of
    /// how large or small each `tt[i]`'s own magnitude is. The
    /// leaked-signal noise above hit exactly that degenerate case,
    /// `sequential_decode_with_confidence_gate()` then accepted the
    /// result as a confident, wrong decode -- a real silent-wrong-answer
    /// failure, this codebase's own fail-fast convention's whole reason
    /// to exist. Fixed by `cosine_score()`'s proper L2-normalized
    /// correlation coefficient (see its own doc comment), which is
    /// smoothly graded rather than saturating, and by `MIN_SYNC_SCORE`
    /// gating `find_sync()`/`sync_search_and_decode()` directly.
    ///
    /// This test locks in the fix for both, with the corrected
    /// (genuinely independent) noise methodology: `find_sync()` must
    /// reject it outright (`None`, via `MIN_SYNC_SCORE`), and even
    /// granting the raw search its own best-effort candidate below that
    /// gate, the full decode pipeline must not return a confident
    /// `Ok(..)` for it either -- two independent layers, not one.
    #[test]
    fn find_sync_and_decode_correctly_reject_genuinely_independent_noise() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let sample_rate = 12000u32;
        let true_hz = 1503.2;
        let spsym = samples_per_symbol(sample_rate);

        let tx = wspr_modulate(&symbols, true_hz, sample_rate);
        let mut clean_audio = vec![0i16; 3 * spsym];
        clean_audio.extend_from_slice(&tx);
        clean_audio.extend_from_slice(&[0i16; 4]);
        let signal_sync = find_sync(
            &clean_audio,
            sample_rate,
            1490.0,
            1510.0,
            5 * spsym,
            MIN_SYNC_SCORE,
        )
        .expect("the real signal must be found");

        // Genuinely independent noise: add_awgn() against a constant,
        // non-signal reference (nonzero so add_awgn()'s own signal_power
        // isn't degenerate zero), with that reference subtracted back
        // out -- no mathematical relationship to the real WSPR
        // transmission above, unlike `noisy - clean` (see this test's
        // own doc comment for why that distinction is the whole point).
        let reference = vec![10000i16; clean_audio.len()];
        let with_noise = add_awgn(&reference, 0.0, 99);
        let independent_noise: Vec<i16> = with_noise
            .iter()
            .zip(reference.iter())
            .map(|(&n, &r)| (n as i32 - r as i32) as i16)
            .collect();

        let gated = find_sync(
            &independent_noise,
            sample_rate,
            1490.0,
            1510.0,
            5 * spsym,
            MIN_SYNC_SCORE,
        );
        assert!(
            gated.is_none(),
            "find_sync() must reject genuinely independent noise via MIN_SYNC_SCORE, got {gated:?}"
        );

        // Second, independent layer: even granting the raw (ungated)
        // search its own best-effort candidate, the decode pipeline
        // must not confidently accept it either.
        let ungated = find_sync(
            &independent_noise,
            sample_rate,
            1490.0,
            1510.0,
            5 * spsym,
            f64::NEG_INFINITY,
        )
        .expect("the raw search always returns its own best-scoring candidate when ungated");
        assert!(
            ungated.sync_score < signal_sync.sync_score - 0.5,
            "a real signal's own sync_score ({}) should be well clear of independent noise's own \
             best-scoring false candidate ({})",
            signal_sync.sync_score,
            ungated.sync_score
        );
        let evidence = extract_symbol_evidence(
            &independent_noise,
            sample_rate,
            ungated.base_hz,
            ungated.start_sample,
        )
        .expect("the noise buffer is long enough for a full window at its own winning candidate");
        let decode_result = decode_from_symbol_evidence(
            &evidence,
            2_000_000,
            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
        );
        assert!(
            decode_result.is_err(),
            "independent noise with no real transmission must not decode as if it were a confident, \
             correct message, got {decode_result:?} (ungated={ungated:?})"
        );

        // Third, independent layer, at the real top-level entry point:
        // even ungated end to end, sync_search_and_decode_message() must
        // not surface a plausible-looking (callsign, grid, power)
        // message for pure noise -- whichever of ConfidenceGateError's
        // reasons or WsprMessageError::UnpackFailed actually catches it.
        let message_result = sync_search_and_decode_message(
            &independent_noise,
            sample_rate,
            1490.0,
            1510.0,
            5 * spsym,
            f64::NEG_INFINITY,
            2_000_000,
            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
        )
        .expect("the raw search always returns its own best-scoring candidate when ungated");
        // Measured, not assumed: for this exact noise realization, the
        // decoder's own confidence gate (LowConfidence) is the layer
        // that actually catches it, not unpack_wspr_message()'s power-
        // range check -- both layers exist as independent, real
        // defenses (see WsprMessageError::UnpackFailed's own doc
        // comment for why the power check is real, not redundant), this
        // particular case just didn't need the second one.
        assert!(
            message_result.is_err(),
            "independent noise must not surface as a plausible-looking decoded message, got {message_result:?}"
        );
    }

    /// End-to-end: real search + real evidence extraction + real
    /// decode, on a clean fixture, recovers the exact original message
    /// -- the actual goal of this module, not just its own internal
    /// pieces in isolation.
    #[test]
    fn sync_search_and_decode_recovers_the_exact_message_on_a_clean_signal() {
        let symbols = wspr_encode_symbols("K1ABC", "EM10", 23).unwrap();
        let sample_rate = 12000u32;
        let true_hz = 1497.6;
        let spsym = samples_per_symbol(sample_rate);
        let audio = wspr_modulate(&symbols, true_hz, sample_rate);
        let mut padded = vec![0i16; 2 * spsym];
        padded.extend_from_slice(&audio);
        padded.extend_from_slice(&[0i16; 4]);

        let (base_hz, result) = sync_search_and_decode(
            &padded,
            sample_rate,
            1490.0,
            1505.0,
            4 * spsym,
            MIN_SYNC_SCORE,
            2_000_000,
            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
        )
        .expect("a real clean signal must be found");
        let decoded_bits = result.expect("a clean signal should decode with high confidence");

        let expected = expected_decodable_bits("K1ABC", "EM10", 23);
        assert_eq!(decoded_bits, expected);
        // The detected base_hz must track the real injected frequency
        // (true_hz above), not just decode correctly despite it --
        // AUTO_TUNE_AND_MODE_DETECTION.md's own tuning-correction use
        // needs this value to be accurate, not just present.
        assert!(
            (base_hz - true_hz).abs() < 1.0,
            "base_hz {base_hz} should be within 1Hz of the real injected {true_hz}"
        );
    }

    /// The same end-to-end path, but with real additive Gaussian noise
    /// injected at a moderate level (matching `wspr_decode.rs`'s own
    /// moderate-noise decoder test) -- confirms the search and evidence
    /// extraction still work when the signal isn't clean, not just that
    /// the decoder alone tolerates noise given a perfect sync hypothesis.
    #[test]
    fn sync_search_and_decode_recovers_the_message_at_moderate_noise() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let sample_rate = 12000u32;
        let true_hz = 1501.1;
        let spsym = samples_per_symbol(sample_rate);
        let clean = wspr_modulate(&symbols, true_hz, sample_rate);
        let mut padded = vec![0i16; 2 * spsym];
        padded.extend_from_slice(&clean);
        padded.extend_from_slice(&[0i16; 4]);
        let noisy = add_awgn(&padded, -20.0, 7);

        let (base_hz, result) = sync_search_and_decode(
            &noisy,
            sample_rate,
            1490.0,
            1505.0,
            4 * spsym,
            MIN_SYNC_SCORE,
            2_000_000,
            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
        )
        .expect("a real, moderately noisy signal must still be found");
        let decoded_bits = result.expect("a moderately noisy signal should still decode");

        let expected = expected_decodable_bits("K6BP", "CM87", 30);
        assert_eq!(decoded_bits, expected);
        assert!(
            (base_hz - true_hz).abs() < 1.0,
            "base_hz {base_hz} should be within 1Hz of the real injected {true_hz}"
        );
    }

    /// The real, full pipeline this build order's step 4 exists for:
    /// raw (moderately noisy) audio in, the exact original human-
    /// readable message out -- not just the raw bitset the two tests
    /// above stop at.
    #[test]
    fn sync_search_and_decode_message_recovers_the_exact_original_text() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let sample_rate = 12000u32;
        let true_hz = 1501.1;
        let spsym = samples_per_symbol(sample_rate);
        let clean = wspr_modulate(&symbols, true_hz, sample_rate);
        let mut padded = vec![0i16; 2 * spsym];
        padded.extend_from_slice(&clean);
        padded.extend_from_slice(&[0i16; 4]);
        let noisy = add_awgn(&padded, -20.0, 7);

        let result = sync_search_and_decode_message(
            &noisy,
            sample_rate,
            1490.0,
            1505.0,
            4 * spsym,
            MIN_SYNC_SCORE,
            2_000_000,
            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
        )
        .expect("a real, moderately noisy signal must still be found");
        let (callsign, grid, power, base_hz) =
            result.expect("a moderately noisy signal should still decode to a valid message");

        assert_eq!(
            (callsign, grid, power),
            ("K6BP".to_string(), "CM87".to_string(), 30)
        );
        assert!(
            (base_hz - true_hz).abs() < 1.0,
            "base_hz {base_hz} should be within 1Hz of the real injected {true_hz}"
        );
    }

    /// The isolating test `decimate_4x_box_average()`'s own doc comment
    /// calls for: separates "does the decimator work" from "does the
    /// real-time pipeline's slot logic work" (a future
    /// `hams_local_relay` change, tested there, not here) -- a real
    /// WSPR fixture synthesized at 48kHz (this daemon's own real mic
    /// rate), decimated to 12kHz, must still decode correctly through
    /// this crate's already-tested 12kHz path.
    #[test]
    fn decimate_then_sync_search_and_decode_message_recovers_the_message_from_48khz_audio() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let sample_rate_48k = 48000u32;
        let true_hz = 1501.1;
        let audio_48k = wspr_modulate(&symbols, true_hz, sample_rate_48k);

        // Pad in the 48kHz domain, in multiples of 4, so decimation
        // lands on clean 12kHz-domain boundaries.
        let pad_symbols_48k = 2 * samples_per_symbol(sample_rate_48k);
        let mut padded_48k = vec![0i16; pad_symbols_48k];
        padded_48k.extend_from_slice(&audio_48k);
        padded_48k.extend_from_slice(&[0i16; 16]);

        let decimated_12k = decimate_4x_box_average(&padded_48k);
        let sample_rate_12k = 12000u32;
        let spsym_12k = samples_per_symbol(sample_rate_12k);

        let result = sync_search_and_decode_message(
            &decimated_12k,
            sample_rate_12k,
            1490.0,
            1505.0,
            4 * spsym_12k,
            MIN_SYNC_SCORE,
            2_000_000,
            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
        )
        .expect("a real, clean 48kHz-synthesized-then-decimated signal must be found");
        let (callsign, grid, power, _base_hz) =
            result.expect("a decimated clean signal should decode to a valid message");

        assert_eq!(
            (callsign, grid, power),
            ("K6BP".to_string(), "CM87".to_string(), 30)
        );
    }

    /// Diagnostic, not a correctness assertion: measures how much
    /// `find_sync()`'s own search cost changes between a correct
    /// `max_start_sample` (`buffer_len - required_window_samples()`) and
    /// the bug `digital_decoder.rs`'s real pipeline wiring shipped with
    /// (`buffer_len - 1`), on a buffer the size a real ~125s WSPR
    /// accumulation window produces. Run manually with `--ignored
    /// --nocapture` -- see this module's own doc comment on
    /// `required_window_samples()` for why this matters.
    #[test]
    #[ignore]
    fn diagnostic_max_start_sample_cost_comparison() {
        let sample_rate = 12000u32;
        let buffer_len = 125 * sample_rate as usize; // ~125s, matching a real accumulation window.
        let samples = vec![0i16; buffer_len];
        let correct_max_start = buffer_len.saturating_sub(required_window_samples(sample_rate));
        let buggy_max_start = buffer_len.saturating_sub(1);

        let t0 = std::time::Instant::now();
        let _ = find_sync(
            &samples,
            sample_rate,
            1400.0,
            1600.0,
            correct_max_start,
            MIN_SYNC_SCORE,
        );
        let correct_elapsed = t0.elapsed();

        let t1 = std::time::Instant::now();
        let _ = find_sync(
            &samples,
            sample_rate,
            1400.0,
            1600.0,
            buggy_max_start,
            MIN_SYNC_SCORE,
        );
        let buggy_elapsed = t1.elapsed();

        println!(
            "correct max_start_sample={correct_max_start} took {correct_elapsed:?}; \
             buggy max_start_sample={buggy_max_start} took {buggy_elapsed:?}"
        );
    }

    /// Diagnostic: sizes step 6's own noise-ladder sweep (this file's
    /// own end-to-end decoder vs. the real `wsprd` reference decoder's
    /// already-recorded -30/-34.5dB boundary, `WSPR_DECODE_IMPLEMENTATION_
    /// PLAN.md`'s own step 6 notes) by timing one full-band (1400-1600Hz,
    /// the same band `digital_decoder.rs`'s real pipeline searches, not
    /// a narrowed one) decode on a fixture padded with a realistic
    /// amount of search room -- a bare 110.6s transmission has only one
    /// valid start offset, which would make the sweep's own timing an
    /// unrealistically optimistic stand-in for the live buffer.
    #[test]
    #[ignore]
    fn diagnostic_full_band_decode_timing_with_realistic_padding() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let noisy = add_awgn(&clean, -25.0, 1);
        let pad = 5 * sample_rate as usize; // ~5s each side, matching digital_decoder.rs's own real slot-boundary tail retention.
        let mut padded = vec![0i16; pad];
        padded.extend_from_slice(&noisy);
        padded.extend(vec![0i16; pad]);

        let max_start_sample = padded
            .len()
            .saturating_sub(required_window_samples(sample_rate));
        let t0 = std::time::Instant::now();
        let result = sync_search_and_decode_message(
            &padded,
            sample_rate,
            1400.0,
            1600.0,
            max_start_sample,
            MIN_SYNC_SCORE,
            2_000_000,
            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
        );
        let elapsed = t0.elapsed();
        println!(
            "full-band decode on a {}-sample buffer (max_start_sample={max_start_sample}) took {elapsed:?}, result={result:?}",
            padded.len()
        );
    }

    /// Step 6 of `WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s own build order:
    /// real verification, not just "it compiles" -- sweeps this crate's
    /// own end-to-end decoder (`sync_search_and_decode_message()`, full
    /// 1400-1600Hz band, realistic search-window padding, exactly the
    /// path `digital_decoder.rs`'s live pipeline runs) across the same
    /// AWGN noise ladder whose SNR levels already carry a recorded real
    /// `wsprd` reference-decoder boundary (`WSPR_DECODE_IMPLEMENTATION_
    /// PLAN.md`'s own table: decodes correctly through -32.5dB,
    /// seed-dependent -33.0 to -34.0dB, fails at -34.5dB and below).
    /// This is the comparison the doc's own "what this document
    /// deliberately does not claim" section flags as open: whether the
    /// independently-written sequential decoder + sync search silently
    /// underperforms the battle-tested reference rather than failing
    /// loudly. Three-way classification per fixture, not just
    /// decode-or-not -- `wsprd`'s own recorded property is that it
    /// "either decoded to the exact correct message or produced no
    /// decode at all -- never a wrong message," and that is exactly
    /// the property this decoder's own three independent gates
    /// (`MIN_SYNC_SCORE`, the decoder's confidence gate,
    /// `unpack_grid4_power()`'s range validation) exist to provide, so
    /// a wrong-message result here is a hard test failure, not just a
    /// data point -- while a no-decode at low SNR is an expected,
    /// non-failing outcome given a real, honestly-scoped decoder isn't
    /// guaranteed to match a reference implementation's every last
    /// dB of sensitivity.
    ///
    /// Real, deliberate limit on what this closes (see this module's
    /// own `WSPR_DECODE_IMPLEMENTATION_PLAN.md` step 6 notes): AWGN is
    /// synthetic noise on an otherwise-clean signal, not a genuine
    /// off-air capture -- this does not exercise real frequency drift,
    /// non-Gaussian real-world noise, or the adjacent-transmission
    /// false-sync risk this module's own doc comment carries forward
    /// as still unaddressed. This closes the "does our own decoder's
    /// accuracy hold up against the reference" half of step 6, not the
    /// "does it work on real captures" half.
    ///
    /// ~10-11 minutes at `--release` (8 SNR levels x 5 seeds x ~16s
    /// each, per `diagnostic_full_band_decode_timing_with_realistic_
    /// padding`'s own measurement) -- run manually with `--ignored
    /// --nocapture`.
    #[test]
    #[ignore]
    fn own_decoder_noise_ladder_matches_the_recorded_wsprd_reference_boundary() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let pad = 5 * sample_rate as usize;

        #[derive(Debug, PartialEq)]
        enum Outcome {
            Correct,
            NoDecode,
            WrongMessage(String, String, i32),
        }

        // Spans wsprd's own recorded boundary (decodes through -32.5,
        // seed-dependent -33.0 to -34.0, fails at -34.5) plus one easy
        // reference point and one point past the known failure floor.
        let snr_levels = [
            -20.0, -30.0, -31.5, -32.5, -33.0, -33.5, -34.0, -34.5, -36.0,
        ];
        let seeds = [1u64, 2, 3, 4, 5];

        let mut table = Vec::new();
        let mut any_wrong = false;
        for &snr_db in &snr_levels {
            let mut correct = 0;
            let mut no_decode = 0;
            for &seed in &seeds {
                let noisy = add_awgn(&clean, snr_db, seed);
                let mut padded = vec![0i16; pad];
                padded.extend_from_slice(&noisy);
                padded.extend(vec![0i16; pad]);
                let max_start_sample = padded
                    .len()
                    .saturating_sub(required_window_samples(sample_rate));

                let result = sync_search_and_decode_message(
                    &padded,
                    sample_rate,
                    1400.0,
                    1600.0,
                    max_start_sample,
                    MIN_SYNC_SCORE,
                    2_000_000,
                    crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                );
                let outcome = match result {
                    Some(Ok((callsign, grid, power, _base_hz)))
                        if callsign == "K6BP" && grid == "CM87" && power == 30 =>
                    {
                        Outcome::Correct
                    }
                    Some(Ok((callsign, grid, power, _base_hz))) => {
                        Outcome::WrongMessage(callsign, grid, power)
                    }
                    Some(Err(_)) | None => Outcome::NoDecode,
                };
                match outcome {
                    Outcome::Correct => correct += 1,
                    Outcome::NoDecode => no_decode += 1,
                    Outcome::WrongMessage(ref c, ref g, p) => {
                        any_wrong = true;
                        println!("WRONG MESSAGE at {snr_db}dB seed={seed}: got ({c}, {g}, {p}), expected (K6BP, CM87, 30)");
                    }
                }
            }
            table.push((
                snr_db,
                correct,
                no_decode,
                seeds.len() - correct - no_decode,
            ));
        }

        println!("SNR(dB) | correct | no-decode | wrong");
        for (snr_db, correct, no_decode, wrong) in &table {
            println!("{snr_db:>7} | {correct:>7} | {no_decode:>9} | {wrong:>5}");
        }

        assert!(!any_wrong, "own decoder must never produce a confident wrong message, same property wsprd's own recorded ladder has");
    }

    /// Diagnostic: isolates candidate cause 1 (`find_sync()`'s own coarse
    /// discrete search grid) from candidates 2/3 (amplitude/noise_stddev
    /// calibration, decoder metric/budget tuning) for the ~2.5-4dB
    /// sensitivity gap `own_decoder_noise_ladder_matches_the_recorded_
    /// wsprd_reference_boundary` measured against the real `wsprd`
    /// reference. Bypasses `find_sync()` entirely -- calls `extract_
    /// symbol_evidence()` directly with the exact, hand-known `base_hz`/
    /// `start_sample` the fixture was built at (no search, no discrete-grid
    /// quantization error possible) -- then runs the same decode path
    /// (`decode_from_symbol_evidence()`) the full pipeline uses. If this
    /// sensitivity boundary is close to `wsprd`'s own (through -32.5dB, not
    /// falling off at -30dB the way the full search does), the sync
    /// search's own coarse grid is the dominant cause of the gap and
    /// candidates 2/3 are not the story; if this ALSO falls off around
    /// -30/-31.5dB, the gap is downstream of sync (calibration or decoder
    /// tuning), and `find_sync()` itself is not the problem to fix first.
    #[test]
    #[ignore]
    fn own_decoder_noise_ladder_with_exact_known_sync_isolates_the_sensitivity_gap_cause() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let true_base_hz = 1500.0;
        let true_start_sample = 0usize; // no padding -- exact alignment is hand-known, not searched for.

        let snr_levels = [
            -20.0, -30.0, -31.5, -32.5, -33.0, -33.5, -34.0, -34.5, -36.0,
        ];
        let seeds = [1u64, 2, 3, 4, 5];

        let mut table = Vec::new();
        let mut any_wrong = false;
        for &snr_db in &snr_levels {
            let mut correct = 0;
            let mut no_decode = 0;
            for &seed in &seeds {
                let noisy = add_awgn(&clean, snr_db, seed);
                let evidence =
                    extract_symbol_evidence(&noisy, sample_rate, true_base_hz, true_start_sample)
                        .expect(
                        "exact-known alignment on a fixture this long must always yield evidence",
                    );
                let result = decode_from_symbol_evidence(
                    &evidence,
                    2_000_000,
                    crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                )
                .ok()
                .and_then(unpack_wspr_message);
                match result {
                    Some((callsign, grid, power))
                        if callsign == "K6BP" && grid == "CM87" && power == 30 =>
                    {
                        correct += 1
                    }
                    Some((callsign, grid, power)) => {
                        any_wrong = true;
                        println!("WRONG MESSAGE at {snr_db}dB seed={seed}: got ({callsign}, {grid}, {power}), expected (K6BP, CM87, 30)");
                    }
                    None => no_decode += 1,
                }
            }
            table.push((
                snr_db,
                correct,
                no_decode,
                seeds.len() - correct - no_decode,
            ));
        }

        println!("exact-known-sync SNR(dB) | correct | no-decode | wrong");
        for (snr_db, correct, no_decode, wrong) in &table {
            println!("{snr_db:>7} | {correct:>7} | {no_decode:>9} | {wrong:>5}");
        }

        assert!(
            !any_wrong,
            "own decoder must never produce a confident wrong message even with exact known sync"
        );
    }

    /// Diagnostic: fast K (window half-width) sweep at just the two
    /// rungs right past the ladder's own real failure boundary (-30,
    /// -31.5dB), exact known sync (bypasses `find_sync()` for speed and
    /// to isolate the channel-model variable alone, same rationale as
    /// `own_decoder_noise_ladder_with_exact_known_sync_isolates_the_
    /// sensitivity_gap_cause` above), more seeds than the full ladder
    /// (statistical power is cheap at just 2 SNR levels). Written after
    /// `CHANNEL_ESTIMATE_WINDOW_HALF_WIDTH=8`'s own full-ladder run
    /// (`own_decoder_noise_ladder_matches_the_recorded_wsprd_reference_
    /// boundary`, `hams_open` commit pending) came back 0/5 at -30dB and
    /// below -- NOT better than the pre-redesign baseline (which got 1/5
    /// at -30dB), and arguably slightly worse. This sweeps a real range
    /// of K values (both narrower AND wider than 8) to check whether a
    /// different window size does better, before concluding the windowed
    /// approach itself doesn't help at this specific failure mode.
    #[test]
    #[ignore]
    fn diagnostic_window_half_width_sweep_at_the_failure_boundary() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let true_base_hz = 1500.0;
        let true_start_sample = 0usize;

        let snr_levels = [-30.0, -31.5];
        let seeds: Vec<u64> = (1..=15).collect();
        let window_half_widths = [2usize, 4, 8, 20, 40, 81, 161];

        println!("SNR(dB) | window_half_width | correct/{}", seeds.len());
        for &snr_db in &snr_levels {
            for &half_width in &window_half_widths {
                let mut correct = 0;
                for &seed in &seeds {
                    let noisy = add_awgn(&clean, snr_db, seed);
                    let evidence = extract_symbol_evidence(
                        &noisy,
                        sample_rate,
                        true_base_hz,
                        true_start_sample,
                    )
                    .expect(
                        "exact-known alignment on a fixture this long must always yield evidence",
                    );
                    let impossible_tone_evidence = extract_impossible_tone_evidence(
                        &noisy,
                        sample_rate,
                        true_base_hz,
                        true_start_sample,
                    )
                    .expect(
                        "exact-known alignment on a fixture this long must always yield evidence",
                    );
                    let (symbol_values, amplitude, noise_stddev) =
                        evidence_to_symbol_values_windowed_clean_reference(
                            &evidence,
                            &impossible_tone_evidence,
                            half_width,
                        );
                    let channel_bit_values = deinterleave_symbol_values(&symbol_values);
                    let channel_amplitude = deinterleave_symbol_values(&amplitude);
                    let channel_noise_stddev = deinterleave_symbol_values(&noise_stddev);
                    let result = sequential_decode_with_confidence_gate(
                        &channel_bit_values,
                        &channel_amplitude,
                        &channel_noise_stddev,
                        2_000_000,
                        crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                    )
                    .ok()
                    .and_then(unpack_wspr_message);
                    if let Some((callsign, grid, power)) = result {
                        if callsign == "K6BP" && grid == "CM87" && power == 30 {
                            correct += 1;
                        }
                    }
                }
                println!("{snr_db:>7} | {half_width:>18} | {correct}/{}", seeds.len());
            }
        }
    }

    /// Diagnostic: the design doc's OTHER candidate local noise
    /// estimator -- windowed winner/loser variance (`sqrt((winvar+
    /// losevar)/2)` over a local window, same formula the pre-redesign
    /// global scalar used, just localized) -- since the windowed
    /// clean-noise-reference candidate above was just measured to fail
    /// at every window size, and a direct ratio check
    /// (`diagnostic_clean_noise_reference_from_impossible_tones`) ruled
    /// out a scale bug as the explanation (ratio stayed within ~0.97-1.14
    /// of the raw winner/loser value near the failure boundary, not a
    /// 2x-or-more scale mismatch). Also sweeps the summed-variance
    /// correction (`sqrt(winvar+losevar)`, no `/2`) and the amplitude
    /// debias term (`amplitude - k*noise_stddev_raw`) the doc's own
    /// "Windowed winner/loser estimate" section says this candidate
    /// would need -- both already validated in DIRECTION (not magnitude)
    /// by the pre-redesign global `diagnostic_noise_stddev_scale_factor_
    /// sweep_at_the_failure_boundary`/`diagnostic_combined_correction_
    /// sweep_against_full_ladder` diagnostics, but never combined with
    /// real per-symbol windowing until now.
    #[test]
    #[ignore]
    fn diagnostic_windowed_winner_loser_estimator_sweep_at_the_failure_boundary() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let true_base_hz = 1500.0;
        let true_start_sample = 0usize;

        let snr_levels = [-30.0, -31.5];
        let seeds: Vec<u64> = (1..=15).collect();
        let window_half_widths = [8usize, 20, 40];
        // (sum_variance, debias_k) -- sum_variance=false reproduces the
        // OLD /2 formula, just windowed; sum_variance=true is the
        // sqrt(2)-ish correction the scale-factor sweep found helpful.
        let corrections: [(bool, f64); 4] =
            [(false, 0.0), (true, 0.0), (false, 1.13), (true, 1.13)];

        println!(
            "SNR(dB) | half_width | sum_var | debias_k | correct/{}",
            seeds.len()
        );
        for &snr_db in &snr_levels {
            for &half_width in &window_half_widths {
                for &(sum_variance, debias_k) in &corrections {
                    let mut correct = 0;
                    for &seed in &seeds {
                        let noisy = add_awgn(&clean, snr_db, seed);
                        let evidence = extract_symbol_evidence(&noisy, sample_rate, true_base_hz, true_start_sample)
                            .expect("exact-known alignment on a fixture this long must always yield evidence");

                        let mut symbol_values = [0.0f64; WSPR_NUM_SYMBOLS];
                        let mut winners = [0.0f64; WSPR_NUM_SYMBOLS];
                        let mut losers = [0.0f64; WSPR_NUM_SYMBOLS];
                        for i in 0..WSPR_NUM_SYMBOLS {
                            let v0 = evidence[i][0];
                            let v1 = evidence[i][1];
                            symbol_values[i] = v1 - v0;
                            winners[i] = v0.max(v1);
                            losers[i] = v0.min(v1);
                        }

                        let mut amplitude = [0.0f64; WSPR_NUM_SYMBOLS];
                        let mut noise_stddev = [0.0f64; WSPR_NUM_SYMBOLS];
                        for i in 0..WSPR_NUM_SYMBOLS {
                            let lo = i.saturating_sub(half_width);
                            let hi = (i + half_width).min(WSPR_NUM_SYMBOLS - 1);
                            let n = (hi - lo + 1) as f64;
                            let winmean = winners[lo..=hi].iter().sum::<f64>() / n;
                            let losemean = losers[lo..=hi].iter().sum::<f64>() / n;
                            let winvar = winners[lo..=hi]
                                .iter()
                                .map(|x| (x - winmean) * (x - winmean))
                                .sum::<f64>()
                                / n;
                            let losevar = losers[lo..=hi]
                                .iter()
                                .map(|x| (x - losemean) * (x - losemean))
                                .sum::<f64>()
                                / n;
                            let amplitude_raw = (winmean - losemean).max(1e-9);
                            let noise_stddev_raw = if sum_variance {
                                (winvar + losevar).sqrt().max(1e-9)
                            } else {
                                ((winvar + losevar) / 2.0).sqrt().max(1e-9)
                            };
                            amplitude[i] = (amplitude_raw
                                - debias_k * ((winvar + losevar) / 2.0).sqrt())
                            .max(1e-9);
                            noise_stddev[i] = noise_stddev_raw;
                        }

                        let channel_bit_values = deinterleave_symbol_values(&symbol_values);
                        let channel_amplitude = deinterleave_symbol_values(&amplitude);
                        let channel_noise_stddev = deinterleave_symbol_values(&noise_stddev);
                        let result = sequential_decode_with_confidence_gate(
                            &channel_bit_values,
                            &channel_amplitude,
                            &channel_noise_stddev,
                            2_000_000,
                            crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                        )
                        .ok()
                        .and_then(unpack_wspr_message);
                        if let Some((callsign, grid, power)) = result {
                            if callsign == "K6BP" && grid == "CM87" && power == 30 {
                                correct += 1;
                            }
                        }
                    }
                    println!(
                        "{snr_db:>7} | {half_width:>10} | {sum_variance:>7} | {debias_k:>8} | {correct}/{}",
                        seeds.len()
                    );
                }
            }
        }
    }

    /// Diagnostic, not a correctness assertion: dumps the (amplitude,
    /// noise_stddev) pair `evidence_to_symbol_values()` actually computes
    /// at each rung of the noise ladder, exact known sync (no search
    /// error), to compare against the noise_stddev/amplitude ratios
    /// `wspr_decode.rs`'s own tests have ever validated the sequential
    /// decoder against (0.5 and 0.9 at amplitude=1.0 -- i.e. ratios of
    /// 0.5 and 0.9 -- plus a deliberately-hopeless 5.0). If the ladder's
    /// own failing SNR levels produce a noise_stddev/amplitude ratio well
    /// past 0.9, the decoder's own metric table/max_cycles budget was
    /// simply never validated that deep -- real, unsurprising
    /// under-tuning, not a mystery bug.
    #[test]
    #[ignore]
    fn diagnostic_dump_calibrated_amplitude_and_noise_stddev_per_snr_rung() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let snr_levels = [-20.0, -30.0, -31.5, -32.5, -33.0, -34.0, -34.5, -36.0];
        println!("SNR(dB) | seed | amplitude | noise_stddev | ratio");
        for &snr_db in &snr_levels {
            for seed in 1u64..=3 {
                let noisy = add_awgn(&clean, snr_db, seed);
                let evidence = extract_symbol_evidence(&noisy, sample_rate, 1500.0, 0).unwrap();
                let (_symbol_values, amplitude, noise_stddev) =
                    evidence_to_symbol_values(&evidence);
                // Per-symbol arrays now (Per-Symbol Channel Model
                // Redesign) -- this historical diagnostic printed one
                // global pair, so mean() across the array preserves its
                // original intent (a single comparable summary number)
                // rather than dumping 162 rows.
                let mean = |xs: &[f64; WSPR_NUM_SYMBOLS]| {
                    xs.iter().sum::<f64>() / (WSPR_NUM_SYMBOLS as f64)
                };
                let (mean_amplitude, mean_noise_stddev) = (mean(&amplitude), mean(&noise_stddev));
                println!(
                    "{snr_db:>7} | {seed:>4} | {mean_amplitude:>9.4} | {mean_noise_stddev:>12.4} | {:>5.3}",
                    mean_noise_stddev / mean_amplitude
                );
            }
        }
    }

    /// Diagnostic: quantifies the order-statistics bias in `evidence_to_
    /// symbol_values()`'s `amplitude` estimate. `winner = max(v0,v1)`,
    /// `loser = min(v0,v1)` per symbol -- in *pure noise* (no real signal
    /// separating v0/v1 at all) `max` minus `min` of two random draws is
    /// still strictly positive on average, so `amplitude = mean(winner) -
    /// mean(loser)` is a biased-upward estimate of the true signal
    /// separation, not a clean noise-floor-crossing measurement. Feeds a
    /// silence-only buffer (zero signal, `add_awgn`'s own noise generator
    /// only) straight into `extract_symbol_evidence()`/`evidence_to_
    /// symbol_values()` and reports the resulting amplitude directly --
    /// any amplitude visibly above the `1e-9` floor confirms the bias
    /// exists and roughly how large it is relative to the real signal
    /// amplitudes the noise-ladder dump above measured (millions, at this
    /// fixture's scale).
    #[test]
    #[ignore]
    fn diagnostic_pure_noise_calibration_reveals_amplitude_bias() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        // -80dB is, for this fixture's amplitude, indistinguishable from
        // "no real signal at all" -- add_awgn scales noise relative to the
        // real signal's own RMS, so a deep enough negative SNR leaves only
        // the noise process itself in the buffer.
        println!("seed | amplitude (should be ~0 if unbiased) | noise_stddev");
        for seed in 1u64..=5 {
            let silence_plus_noise = add_awgn(&clean, -80.0, seed);
            let evidence =
                extract_symbol_evidence(&silence_plus_noise, sample_rate, 1500.0, 0).unwrap();
            let (_symbol_values, amplitude, noise_stddev) = evidence_to_symbol_values(&evidence);
            let mean =
                |xs: &[f64; WSPR_NUM_SYMBOLS]| xs.iter().sum::<f64>() / (WSPR_NUM_SYMBOLS as f64);
            println!(
                "{seed:>4} | {:>12.4} | {:>12.4}",
                mean(&amplitude),
                mean(&noise_stddev)
            );
        }
    }

    /// Diagnostic: splits the exact-known-sync ladder's "no correct
    /// decode" bucket into its three real, distinct failure modes instead
    /// of lumping them as one "no-decode" count. `GaveUp` means the
    /// sequential decoder exhausted `max_cycles` node expansions without
    /// completing a search at all (a budget/search-efficiency problem);
    /// `LowConfidence` means the search completed but the winning path's
    /// own metric fell below `MIN_ACCEPTABLE_METRIC` (a metric-calibration
    /// problem, and `MIN_ACCEPTABLE_METRIC` was itself only ever validated
    /// at noise_stddev/amplitude ratio ~0.9, not the 0.48-0.71 range this
    /// ladder's own failing rungs sit at); `UnpackFailed` means the
    /// decoder returned a confident bit pattern but `unpack_wspr_message()`
    /// rejected it (checksum/field-range failure -- a "confidently wrong
    /// enough to fail unpacking" case, distinct from both). Which bucket
    /// dominates points at a different candidate cause.
    #[test]
    #[ignore]
    fn diagnostic_exact_known_sync_failure_mode_breakdown() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let snr_levels = [-30.0, -31.5, -32.5, -33.0, -34.0, -34.5, -36.0];
        let seeds = [1u64, 2, 3, 4, 5];

        println!("SNR(dB) | correct | gave_up | low_confidence | unpack_failed");
        for &snr_db in &snr_levels {
            let mut correct = 0;
            let mut gave_up = 0;
            let mut low_confidence = 0;
            let mut unpack_failed = 0;
            for &seed in &seeds {
                let noisy = add_awgn(&clean, snr_db, seed);
                let evidence = extract_symbol_evidence(&noisy, sample_rate, 1500.0, 0).unwrap();
                match decode_from_symbol_evidence(
                    &evidence,
                    2_000_000,
                    crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                ) {
                    Ok(bits) => match unpack_wspr_message(bits) {
                        Some((callsign, grid, power))
                            if callsign == "K6BP" && grid == "CM87" && power == 30 =>
                        {
                            correct += 1
                        }
                        Some(_) => unpack_failed += 1, // confidently wrong, but not garbage
                        None => unpack_failed += 1,
                    },
                    Err(ConfidenceGateError::GaveUp { .. }) => gave_up += 1,
                    Err(ConfidenceGateError::LowConfidence { .. }) => low_confidence += 1,
                }
            }
            println!("{snr_db:>7} | {correct:>7} | {gave_up:>7} | {low_confidence:>15} | {unpack_failed:>13}");
        }
    }

    /// Diagnostic: sweeps a multiplicative correction factor on
    /// `noise_stddev` before handing it to the decoder, at the SNR rungs
    /// right past the real failure boundary (-30 through -33dB), to see
    /// whether a corrected channel estimate recovers decodes the
    /// uncorrected calibration misses. This is the empirical test of the
    /// order-statistics theory: `symbol_values[i] = v1[i] - v0[i]` is a
    /// difference of two ~independent noisy bins (variance sums, so
    /// stddev scales by sqrt(2) relative to a single bin), but `noise_
    /// stddev = sqrt((winvar+losevar)/2)` *averages* the two per-bin
    /// variances instead of summing them -- structurally under-reporting
    /// the real symbol_values noise by roughly sqrt(2). A factor near
    /// 1.41 winning here would confirm that specific mechanism (not just
    /// "give the decoder a bigger noise_stddev generically helps").
    #[test]
    #[ignore]
    fn diagnostic_noise_stddev_scale_factor_sweep_at_the_failure_boundary() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let snr_levels = [-30.0, -31.5, -32.5, -33.0];
        let seeds = [1u64, 2, 3, 4, 5];
        let noise_stddev_factors = [0.7, 1.0, 1.2, 1.41, 1.6, 2.0];

        println!("SNR(dB) | noise_stddev_factor | correct/5");
        for &snr_db in &snr_levels {
            for &factor in &noise_stddev_factors {
                let mut correct = 0;
                for &seed in &seeds {
                    let noisy = add_awgn(&clean, snr_db, seed);
                    let evidence = extract_symbol_evidence(&noisy, sample_rate, 1500.0, 0).unwrap();
                    let (symbol_values, amplitude, noise_stddev) =
                        evidence_to_symbol_values(&evidence);
                    let channel_bit_values = deinterleave_symbol_values(&symbol_values);
                    let channel_amplitude = deinterleave_symbol_values(&amplitude);
                    let mut scaled_noise_stddev = noise_stddev;
                    for v in scaled_noise_stddev.iter_mut() {
                        *v *= factor;
                    }
                    let channel_noise_stddev = deinterleave_symbol_values(&scaled_noise_stddev);
                    let result = sequential_decode_with_confidence_gate(
                        &channel_bit_values,
                        &channel_amplitude,
                        &channel_noise_stddev,
                        2_000_000,
                        crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                    )
                    .ok()
                    .and_then(unpack_wspr_message);
                    if let Some((callsign, grid, power)) = result {
                        if callsign == "K6BP" && grid == "CM87" && power == 30 {
                            correct += 1;
                        }
                    }
                }
                println!("{snr_db:>7} | {factor:>19} | {correct}/5");
            }
        }
    }

    /// Diagnostic: tests the combined, mechanistically-derived correction
    /// against the full noise ladder. Two independent corrections, both
    /// motivated by the pure-noise and scale-factor experiments above:
    /// (1) `noise_stddev_corrected = sqrt(winvar + losevar)` -- summing
    /// rather than averaging the two per-bin variances, since `symbol_
    /// values[i] = v1[i] - v0[i]` is a difference of two ~independent
    /// noisy bins (variance sums; the old `sqrt((winvar+losevar)/2)`
    /// under-reports by exactly the sqrt(2) the scale-factor sweep found
    /// helpful). (2) `amplitude_debiased = max(amplitude_raw - k *
    /// noise_stddev_raw, floor)` -- subtracting the order-statistic bias
    /// the pure-noise experiment measured directly (empirically amplitude/
    /// noise_stddev ratio ~1.3-1.4 in pure noise; ~1.13 is the analytic
    /// E[max-min] for two i.i.d. Gaussians in units of their own stddev,
    /// same order of magnitude). Sweeps k over that range to find what the
    /// ladder itself supports, rather than committing to a single
    /// unverified value.
    #[test]
    #[ignore]
    fn diagnostic_combined_correction_sweep_against_full_ladder() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let snr_levels = [-30.0, -31.5, -32.5, -33.0, -33.5, -34.0, -34.5, -36.0];
        let seeds = [1u64, 2, 3, 4, 5];
        let amplitude_debias_ks = [0.0, 1.0, 1.13, 1.3, 1.5];

        println!("SNR(dB) | k | correct/5");
        for &snr_db in &snr_levels {
            for &k in &amplitude_debias_ks {
                let mut correct = 0;
                for &seed in &seeds {
                    let noisy = add_awgn(&clean, snr_db, seed);
                    let evidence = extract_symbol_evidence(&noisy, sample_rate, 1500.0, 0).unwrap();

                    // Re-derive winvar/losevar/amplitude/noise_stddev_raw
                    // directly (evidence_to_symbol_values() itself is not
                    // touched by this diagnostic) to compute the corrected
                    // pair without modifying production code yet.
                    let mut symbol_values = [0.0f64; WSPR_NUM_SYMBOLS];
                    let mut winners = [0.0f64; WSPR_NUM_SYMBOLS];
                    let mut losers = [0.0f64; WSPR_NUM_SYMBOLS];
                    for i in 0..WSPR_NUM_SYMBOLS {
                        let v0 = evidence[i][0];
                        let v1 = evidence[i][1];
                        symbol_values[i] = v1 - v0;
                        winners[i] = v0.max(v1);
                        losers[i] = v0.min(v1);
                    }
                    let mean = |xs: &[f64; WSPR_NUM_SYMBOLS]| {
                        xs.iter().sum::<f64>() / (WSPR_NUM_SYMBOLS as f64)
                    };
                    let variance = |xs: &[f64; WSPR_NUM_SYMBOLS], m: f64| {
                        xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>()
                            / (WSPR_NUM_SYMBOLS as f64)
                    };
                    let winmean = mean(&winners);
                    let losemean = mean(&losers);
                    let winvar = variance(&winners, winmean);
                    let losevar = variance(&losers, losemean);

                    let amplitude_raw = (winmean - losemean).max(1e-9);
                    let noise_stddev_raw = ((winvar + losevar) / 2.0).sqrt().max(1e-9);
                    let noise_stddev_corrected = (winvar + losevar).sqrt().max(1e-9);
                    let amplitude_debiased = (amplitude_raw - k * noise_stddev_raw).max(1e-9);

                    let channel_bit_values = deinterleave_symbol_values(&symbol_values);
                    let result = sequential_decode_with_confidence_gate(
                        &channel_bit_values,
                        &[amplitude_debiased; WSPR_NUM_SYMBOLS],
                        &[noise_stddev_corrected; WSPR_NUM_SYMBOLS],
                        2_000_000,
                        crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                    )
                    .ok()
                    .and_then(unpack_wspr_message);
                    if let Some((callsign, grid, power)) = result {
                        if callsign == "K6BP" && grid == "CM87" && power == 30 {
                            correct += 1;
                        }
                    }
                }
                println!("{snr_db:>7} | {k:>4} | {correct}/5");
            }
        }
    }

    /// Diagnostic: the impossible-tone-pair noise reference. At each
    /// symbol, tones at index `1 - sync_bit` and `3 - sync_bit` can never
    /// be the real transmitted tone (given the already-known sync bit --
    /// `wspr.rs`'s own `symbols[i] = 2*interleaved[i] + SYNC_VECTOR[i]`
    /// means the transmitted tone is always `sync_bit` or `2+sync_bit`).
    /// Their magnitudes are therefore a clean, always-pure-noise sample at
    /// every symbol -- unlike the winner/loser split, which is contaminated
    /// by the max/min order-statistic bias confirmed in earlier diagnostics.
    /// First dumps how this clean reference compares to the old winner/
    /// loser-derived noise_stddev, then re-runs the full ladder using the
    /// clean reference in place of the old estimate (amplitude left as the
    /// raw winner/loser separation, uncorrected) to see whether a cleaner
    /// noise estimate alone -- no debiasing hack -- closes more of the gap
    /// than the previous commit's empirical k-correction did.
    #[test]
    #[ignore]
    fn diagnostic_clean_noise_reference_from_impossible_tones() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let snr_levels = [
            -20.0, -30.0, -31.5, -32.5, -33.0, -33.5, -34.0, -34.5, -36.0,
        ];
        let seeds = [1u64, 2, 3, 4, 5];

        println!("=== comparison at 3 seeds per rung ===");
        println!("SNR(dB) | seed | noise_stddev_raw | noise_stddev_clean | ratio_clean/raw");
        for &snr_db in &snr_levels {
            for seed in 1u64..=3 {
                let noisy = add_awgn(&clean, snr_db, seed);
                let tone_mags =
                    extract_all_four_tone_magnitudes(&noisy, sample_rate, 1500.0, 0).unwrap();

                let mut winners = [0.0f64; WSPR_NUM_SYMBOLS];
                let mut losers = [0.0f64; WSPR_NUM_SYMBOLS];
                let mut clean_noise_samples = Vec::with_capacity(WSPR_NUM_SYMBOLS * 2);
                for i in 0..WSPR_NUM_SYMBOLS {
                    let sync_bit = SYNC_VECTOR[i] as usize;
                    let v0 = tone_mags[i][sync_bit];
                    let v1 = tone_mags[i][2 + sync_bit];
                    winners[i] = v0.max(v1);
                    losers[i] = v0.min(v1);
                    clean_noise_samples.push(tone_mags[i][1 - sync_bit]);
                    clean_noise_samples.push(tone_mags[i][3 - sync_bit]);
                }
                let mean = |xs: &[f64; WSPR_NUM_SYMBOLS]| {
                    xs.iter().sum::<f64>() / (WSPR_NUM_SYMBOLS as f64)
                };
                let variance = |xs: &[f64; WSPR_NUM_SYMBOLS], m: f64| {
                    xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (WSPR_NUM_SYMBOLS as f64)
                };
                let winmean = mean(&winners);
                let losemean = mean(&losers);
                let winvar = variance(&winners, winmean);
                let losevar = variance(&losers, losemean);
                let noise_stddev_raw = ((winvar + losevar) / 2.0).sqrt().max(1e-9);

                let n = clean_noise_samples.len() as f64;
                let clean_mean = clean_noise_samples.iter().sum::<f64>() / n;
                let clean_var = clean_noise_samples
                    .iter()
                    .map(|x| (x - clean_mean) * (x - clean_mean))
                    .sum::<f64>()
                    / n;
                let noise_stddev_clean = clean_var.sqrt().max(1e-9);

                println!(
                    "{snr_db:>7} | {seed:>4} | {noise_stddev_raw:>16.4} | {noise_stddev_clean:>19.4} | {:>6.3}",
                    noise_stddev_clean / noise_stddev_raw
                );
            }
        }

        println!("\n=== full ladder using clean noise reference, raw amplitude ===");
        println!("SNR(dB) | correct/5");
        for &snr_db in &snr_levels {
            let mut correct = 0;
            for &seed in &seeds {
                let noisy = add_awgn(&clean, snr_db, seed);
                let tone_mags =
                    extract_all_four_tone_magnitudes(&noisy, sample_rate, 1500.0, 0).unwrap();

                let mut symbol_values = [0.0f64; WSPR_NUM_SYMBOLS];
                let mut winners = [0.0f64; WSPR_NUM_SYMBOLS];
                let mut losers = [0.0f64; WSPR_NUM_SYMBOLS];
                let mut clean_noise_samples = Vec::with_capacity(WSPR_NUM_SYMBOLS * 2);
                for i in 0..WSPR_NUM_SYMBOLS {
                    let sync_bit = SYNC_VECTOR[i] as usize;
                    let v0 = tone_mags[i][sync_bit];
                    let v1 = tone_mags[i][2 + sync_bit];
                    symbol_values[i] = v1 - v0;
                    winners[i] = v0.max(v1);
                    losers[i] = v0.min(v1);
                    clean_noise_samples.push(tone_mags[i][1 - sync_bit]);
                    clean_noise_samples.push(tone_mags[i][3 - sync_bit]);
                }
                let mean = |xs: &[f64; WSPR_NUM_SYMBOLS]| {
                    xs.iter().sum::<f64>() / (WSPR_NUM_SYMBOLS as f64)
                };
                let winmean = mean(&winners);
                let losemean = mean(&losers);
                let amplitude_raw = (winmean - losemean).max(1e-9);

                let n = clean_noise_samples.len() as f64;
                let clean_mean = clean_noise_samples.iter().sum::<f64>() / n;
                let clean_var = clean_noise_samples
                    .iter()
                    .map(|x| (x - clean_mean) * (x - clean_mean))
                    .sum::<f64>()
                    / n;
                let noise_stddev_clean = clean_var.sqrt().max(1e-9);

                let channel_bit_values = deinterleave_symbol_values(&symbol_values);
                let result = sequential_decode_with_confidence_gate(
                    &channel_bit_values,
                    &[amplitude_raw; WSPR_NUM_SYMBOLS],
                    &[noise_stddev_clean; WSPR_NUM_SYMBOLS],
                    2_000_000,
                    crate::wspr_decode::MIN_ACCEPTABLE_METRIC,
                )
                .ok()
                .and_then(unpack_wspr_message);
                if let Some((callsign, grid, power)) = result {
                    if callsign == "K6BP" && grid == "CM87" && power == 30 {
                        correct += 1;
                    }
                }
            }
            println!("{snr_db:>7} | {correct}/5");
        }
    }

    /// Diagnostic: `hams_open` commit `1e65753b` (`wspr_decode.rs`) proved
    /// `sequential_decode`/`fano_bit_metric` decode correctly 10/10 at the
    /// real failing ratio range (0.48-0.71) when given a PERFECTLY known
    /// channel model -- ruling out a decoder-algorithm bug and narrowing
    /// the real cause to this function's own calibration: the real,
    /// FFT-extracted `symbol_values[i] = v1[i]-v0[i]` distribution at a
    /// failing SNR must deviate from the bipolar+Gaussian(amplitude,
    /// noise_stddev) model in some way the single global ratio doesn't
    /// capture, since the reported ratio alone looks perfectly benign.
    ///
    /// This test checks the most likely candidate directly: FFT bin
    /// magnitudes under AWGN are Rice-distributed (signal+noise) or
    /// Rayleigh-distributed (noise-only), not Gaussian -- a difference of
    /// two such magnitudes is not itself Gaussian, especially once SNR is
    /// low enough that the "winner" bin is sometimes noise-dominated. If
    /// that's the real mechanism, the empirical per-symbol z-scores
    /// (using each symbol's OWN known-correct-bit sign, so this measures
    /// distribution shape, not decode correctness) should show real skew
    /// and/or heavier-than-Gaussian tails (more extreme outliers than a
    /// true Gaussian would produce), which is exactly the kind of thing
    /// that could tank a handful of symbols' own branch metrics hard
    /// enough to sink `sequential_decode`'s cumulative-sum path metric
    /// even when most symbols are individually fine -- consistent with
    /// the 100%-`LowConfidence`/0%-`GaveUp` split already measured
    /// (`diagnostic_exact_known_sync_failure_mode_breakdown`).
    #[test]
    #[ignore]
    fn diagnostic_symbol_value_distribution_shape_at_a_failing_snr_rung() {
        let sample_rate = 12000u32;
        let clean = wspr_encode_audio("K6BP", "CM87", 30, 1500.0, sample_rate).unwrap();
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        // Ground truth per-symbol data bit, in the SAME real-transmission
        // order extract_symbol_evidence()'s [v0,v1] pairs use -- per
        // wspr.rs's own symbols[i] = 2*interleaved[i] + SYNC_VECTOR[i].
        let true_data_bit: Vec<u8> = symbols.iter().map(|&s| (s >> 1) & 1).collect();

        let snr_db = -32.5; // squarely in the failing range per the exact-known-sync ladder.
        let seeds = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let mut zscores: Vec<f64> = Vec::with_capacity(WSPR_NUM_SYMBOLS * seeds.len());
        for &seed in &seeds {
            let noisy = add_awgn(&clean, snr_db, seed);
            let evidence = extract_symbol_evidence(&noisy, sample_rate, 1500.0, 0).unwrap();
            let (symbol_values, amplitude, noise_stddev) = evidence_to_symbol_values(&evidence);
            for i in 0..WSPR_NUM_SYMBOLS {
                // Expected sign per the true bit: data_bit=1 -> +amplitude, data_bit=0 -> -amplitude.
                // amplitude[i]/noise_stddev[i] are now LOCAL per-symbol
                // estimates (Per-Symbol Channel Model Redesign), which is
                // actually the more correct thing to z-score against here.
                let expected_mean = if true_data_bit[i] == 1 {
                    amplitude[i]
                } else {
                    -amplitude[i]
                };
                zscores.push((symbol_values[i] - expected_mean) / noise_stddev[i]);
            }
        }

        let n = zscores.len() as f64;
        let mean = zscores.iter().sum::<f64>() / n;
        let variance = zscores.iter().map(|z| (z - mean) * (z - mean)).sum::<f64>() / n;
        let stddev = variance.sqrt();
        let skewness = zscores
            .iter()
            .map(|z| ((z - mean) / stddev).powi(3))
            .sum::<f64>()
            / n;
        let excess_kurtosis = zscores
            .iter()
            .map(|z| ((z - mean) / stddev).powi(4))
            .sum::<f64>()
            / n
            - 3.0;
        let max_abs_z = zscores.iter().fold(0.0f64, |m, &z| m.max(z.abs()));
        let extreme_count = zscores.iter().filter(|&&z| z.abs() > 3.0).count();
        // For N truly-iid-Gaussian samples, P(|Z|>3) ~= 0.0027 per sample.
        let expected_extreme_count = 0.0027 * n;

        println!("=== symbol_value z-score distribution shape at {snr_db}dB, n={n} ===");
        println!("mean={mean:.4} (Gaussian model implies ~0), stddev={stddev:.4} (implies ~1)");
        println!("skewness={skewness:.4} (Gaussian implies ~0)");
        println!("excess_kurtosis={excess_kurtosis:.4} (Gaussian implies ~0; heavier tails than Gaussian is positive)");
        println!("max |z|={max_abs_z:.4}");
        println!("|z|>3 count: {extreme_count} observed vs {expected_extreme_count:.2} expected under a true Gaussian");
        // Exploratory diagnostic -- prints the real shape rather than
        // asserting a specific verdict, matching this codebase's own
        // established convention for this whole investigation. A reader
        // decides from the printed numbers whether the tail/skew is
        // large enough to explain the gap; no single threshold here is
        // principled enough to hard-assert against.
        //
        // REAL RESULT, measured 2026-09-01: at -32.5dB, skewness=-0.055
        // and excess_kurtosis=-0.053 -- both essentially zero, i.e. the
        // Gaussian SHAPE assumption is fine, not the culprit. But
        // stddev=1.72 (should be ~1.0 if noise_stddev were correctly
        // calibrated against ground truth) -- a real, substantial
        // variance underestimate, on top of everything the earlier
        // calibration experiments already found. CAVEAT on that number:
        // it's pooled across 10 seeds using each seed's OWN amplitude/
        // noise_stddev estimate, and those per-run estimates themselves
        // vary seed-to-seed (amplitude is a biased order statistic, per
        // diagnostic_pure_noise_calibration_reveals_amplitude_bias) --
        // so 1.72 is an upper bound on true per-symbol miscalibration,
        // conflating it with real run-to-run estimation variance, not a
        // clean isolated measurement of per-symbol noise alone. Doesn't
        // change the conclusion below (the scale-factor sweep failure is
        // independent of this pooling, and skew/kurtosis ~0 is robust to
        // it) -- but don't treat "1.72" as more precise than it is; a
        // per-seed breakdown would separate the two components if anyone
        // needs the exact split, not yet run. Re-running
        // `diagnostic_noise_stddev_scale_factor_sweep_at_the_failure_
        // boundary` confirmed even this specific, ground-truth-measured
        // factor (already inside the 0.7-2.0 range that sweep tests)
        // gives 0/5 at -31.5dB and below -- so correcting the SCALAR
        // magnitude of noise_stddev still isn't sufficient, even using
        // the empirically correct value. This narrows the real
        // conclusion further, not just repeats it: the single-global-
        // scalar channel model (one amplitude, one noise_stddev shared
        // across all 162 symbols) is itself too crude to describe real,
        // per-symbol-varying reliability at low SNR -- some symbols'
        // real per-symbol noise is much worse than others' (consistent
        // with FFT-magnitude estimation being inherently per-symbol-
        // variable, not a fixed global quantity), and no single scalar
        // correction to a global noise_stddev can fix that. This is
        // consistent with, and sharpens, this file's earlier "decoder
        // algorithm capability gap" conclusion (see the top-level
        // doc-comment history and night_shift_todo.md) -- but the gap is
        // now precisely located: a per-symbol (not global-scalar)
        // reliability/confidence model is what `wsprd`'s real decoder
        // has and this one doesn't, not a vaguer "the search algorithm
        // itself is worse." A real fix would estimate per-symbol
        // amplitude/noise_stddev (or an equivalent local confidence
        // weight) rather than one number for the whole transmission --
        // a genuine, scoped redesign of `evidence_to_symbol_values()`'s
        // output shape and `fano_bit_metric()`'s inputs, not another
        // scalar-constant sweep.
    }
}
