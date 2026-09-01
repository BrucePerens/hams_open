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
    let tone_mags = tone_magnitudes_from_matrix(&mat, base_bin, base_bin);

    let mut evidence = [[0.0f64; 2]; WSPR_NUM_SYMBOLS];
    for i in 0..WSPR_NUM_SYMBOLS {
        let sync_bit = SYNC_VECTOR[i] as usize;
        evidence[i][0] = tone_mags[i][sync_bit];
        evidence[i][1] = tone_mags[i][2 + sync_bit];
    }
    Some(evidence)
}

/// Converts `extract_symbol_evidence()`'s real, positive tone-magnitude
/// pairs into the bipolar-Gaussian `(symbol_values, amplitude,
/// noise_stddev)` `wspr_decode.rs`'s `sequential_decode()` family
/// expects -- see this module's own doc comment ("Mapping FFT
/// magnitudes...") for the calibration this implements and why it's a
/// heuristic, not a derivation. `symbol_values[i] = v1[i] - v0[i]`
/// (still in real transmission order, matching `extract_symbol_
/// evidence()`'s own convention -- callers pass this through
/// `deinterleave_symbol_values()` before calling the decoder, same as
/// `wspr_decode.rs`'s own tests do). `amplitude`/`noise_stddev` are a
/// single global estimate over all 162 symbols (winner = per-symbol
/// `max(v0,v1)`, loser = per-symbol `min(v0,v1)`; `amplitude = mean
/// (winner) - mean(loser)`, `noise_stddev = sqrt((var(winner) +
/// var(loser)) / 2)`), not `weakmon`'s own per-run local estimates
/// (`statruns`) that track slow SNR drift within one transmission -- a
/// known, deliberate simplification for a first working version.
pub fn evidence_to_symbol_values(
    evidence: &[[f64; 2]; WSPR_NUM_SYMBOLS],
) -> ([f64; WSPR_NUM_SYMBOLS], f64, f64) {
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

    let mean = |xs: &[f64; WSPR_NUM_SYMBOLS]| xs.iter().sum::<f64>() / (WSPR_NUM_SYMBOLS as f64);
    let variance = |xs: &[f64; WSPR_NUM_SYMBOLS], m: f64| {
        xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (WSPR_NUM_SYMBOLS as f64)
    };

    let winmean = mean(&winners);
    let losemean = mean(&losers);
    let winvar = variance(&winners, winmean);
    let losevar = variance(&losers, losemean);

    // Floors: a pathological all-zero or all-equal input (e.g. silence)
    // must not hand the decoder a zero/negative amplitude or a zero
    // noise_stddev -- both would make fano_bit_metric()'s Gaussian
    // shape (and its log2()) undefined or degenerate rather than just
    // producing a low-confidence (correctly rejected) decode.
    let amplitude = (winmean - losemean).max(1e-9);
    let noise_stddev = ((winvar + losevar) / 2.0).sqrt().max(1e-9);

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
    sequential_decode_with_confidence_gate(
        &channel_bit_values,
        amplitude,
        noise_stddev,
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
/// `Some(Err(...))`, not `None` -- "no signal found in this audio" and
/// "found a signal but couldn't decode it" are different, real outcomes
/// a caller may want to distinguish.
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
) -> Option<Result<u128, ConfidenceGateError>> {
    let sync = find_sync(samples, sample_rate, freq_lo_hz, freq_hi_hz, max_start_sample, min_sync_score)?;
    let evidence = extract_symbol_evidence(samples, sample_rate, sync.base_hz, sync.start_sample)?;
    Some(decode_from_symbol_evidence(&evidence, max_cycles, min_acceptable_metric))
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
            ConfidenceGateError::LowConfidence { bits, metric } => WsprMessageError::LowConfidence { bits, metric },
        }
    }
}

/// `sync_search_and_decode()` plus `wspr.rs`'s own `unpack_wspr_
/// message()` -- the real end-to-end entry point this build order's
/// step 4 exists for: raw audio in, a human-readable `(callsign, grid4,
/// power_dbm)` out, or a real, distinguishable failure reason. The
/// function a future `digital_decoder.rs` integration (step 5) should
/// actually call.
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
) -> Option<Result<(String, String, i32), WsprMessageError>> {
    let result = sync_search_and_decode(
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
        Ok(bits) => crate::wspr::unpack_wspr_message(bits).ok_or(WsprMessageError::UnpackFailed { bits }),
        Err(e) => Err(e.into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wspr::{add_awgn, pack_call, pack_grid4_power, wspr_encode_symbols, wspr_modulate};
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
        let mat = spectrum_matrix(&audio, sample_rate, fft.as_ref(), base_bin, base_bin + 3, 0.0, 0).unwrap();
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
            assert_eq!(recovered_bit, expected_data_bit, "symbol {i}: v0={v0} v1={v1}, expected data bit {expected_data_bit}");
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
            let recovered_bit = if evidence[i][1] > evidence[i][0] { 1u8 } else { 0u8 };
            assert_eq!(recovered_bit, expected_data_bit, "symbol {i} at an off-grid frequency");
        }
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

        let sync = find_sync(&audio, sample_rate, 1490.0, 1510.0, 5 * spsym, MIN_SYNC_SCORE)
            .expect("a real, clean, in-band signal must be found");

        let bin_hz = WSPR_SYMBOL_RATE_HZ;
        assert!((sync.base_hz - true_hz).abs() < bin_hz / 2.0, "recovered frequency {} too far from the real {true_hz}", sync.base_hz);
        let true_start_sample = true_start_symbols_padding * spsym;
        assert!(
            (sync.start_sample as i64 - true_start_sample as i64).unsigned_abs() <= (spsym / 8) as u64,
            "recovered start_sample {} too far from the real {true_start_sample}", sync.start_sample
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
        let signal_sync = find_sync(&clean_audio, sample_rate, 1490.0, 1510.0, 5 * spsym, MIN_SYNC_SCORE)
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

        let gated = find_sync(&independent_noise, sample_rate, 1490.0, 1510.0, 5 * spsym, MIN_SYNC_SCORE);
        assert!(gated.is_none(), "find_sync() must reject genuinely independent noise via MIN_SYNC_SCORE, got {gated:?}");

        // Second, independent layer: even granting the raw (ungated)
        // search its own best-effort candidate, the decode pipeline
        // must not confidently accept it either.
        let ungated = find_sync(&independent_noise, sample_rate, 1490.0, 1510.0, 5 * spsym, f64::NEG_INFINITY)
            .expect("the raw search always returns its own best-scoring candidate when ungated");
        assert!(
            ungated.sync_score < signal_sync.sync_score - 0.5,
            "a real signal's own sync_score ({}) should be well clear of independent noise's own \
             best-scoring false candidate ({})",
            signal_sync.sync_score, ungated.sync_score
        );
        let evidence = extract_symbol_evidence(&independent_noise, sample_rate, ungated.base_hz, ungated.start_sample)
            .expect("the noise buffer is long enough for a full window at its own winning candidate");
        let decode_result = decode_from_symbol_evidence(&evidence, 2_000_000, crate::wspr_decode::MIN_ACCEPTABLE_METRIC);
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

        let result = sync_search_and_decode(&padded, sample_rate, 1490.0, 1505.0, 4 * spsym, MIN_SYNC_SCORE, 2_000_000, crate::wspr_decode::MIN_ACCEPTABLE_METRIC)
            .expect("a real clean signal must be found");
        let decoded_bits = result.expect("a clean signal should decode with high confidence");

        let expected = expected_decodable_bits("K1ABC", "EM10", 23);
        assert_eq!(decoded_bits, expected);
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

        let result = sync_search_and_decode(&noisy, sample_rate, 1490.0, 1505.0, 4 * spsym, MIN_SYNC_SCORE, 2_000_000, crate::wspr_decode::MIN_ACCEPTABLE_METRIC)
            .expect("a real, moderately noisy signal must still be found");
        let decoded_bits = result.expect("a moderately noisy signal should still decode");

        let expected = expected_decodable_bits("K6BP", "CM87", 30);
        assert_eq!(decoded_bits, expected);
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

        let result = sync_search_and_decode_message(&noisy, sample_rate, 1490.0, 1505.0, 4 * spsym, MIN_SYNC_SCORE, 2_000_000, crate::wspr_decode::MIN_ACCEPTABLE_METRIC)
            .expect("a real, moderately noisy signal must still be found");
        let message = result.expect("a moderately noisy signal should still decode to a valid message");

        assert_eq!(message, ("K6BP".to_string(), "CM87".to_string(), 30));
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
        let message = result.expect("a decimated clean signal should decode to a valid message");

        assert_eq!(message, ("K6BP".to_string(), "CM87".to_string(), 30));
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
        let _ = find_sync(&samples, sample_rate, 1400.0, 1600.0, correct_max_start, MIN_SYNC_SCORE);
        let correct_elapsed = t0.elapsed();

        let t1 = std::time::Instant::now();
        let _ = find_sync(&samples, sample_rate, 1400.0, 1600.0, buggy_max_start, MIN_SYNC_SCORE);
        let buggy_elapsed = t1.elapsed();

        println!(
            "correct max_start_sample={correct_max_start} took {correct_elapsed:?}; \
             buggy max_start_sample={buggy_max_start} took {buggy_elapsed:?}"
        );
    }
}
