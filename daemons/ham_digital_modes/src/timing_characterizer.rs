//! Mode-agnostic timing/symbol-clock characterization -- a front-end classifier that asks "does
//! this audio have a periodic symbol clock, and at what rate?" *before* committing to decoding
//! it as any particular mode. Meant to sit ahead of the RTTY auto-tune bank (and eventually
//! other digital-mode banks): a candidate frequency with no detectable symbol clock at all can
//! be skipped without ever running a real decoder over it.
//!
//! The core primitive, [`dominant_symbol_period`], is generic over a scalar "feature" time
//! series -- it doesn't know or care whether that feature came from a two-tone FSK ratio (RTTY)
//! or an on/off envelope (CW, Hellschreiber). Two feature extractors are provided:
//! [`two_tone_ratio_feature`] for FSK-ish signals and [`envelope_feature`] for on/off-keyed
//! ones. Mode-specific recognizers are just "extract the right feature, then hand it to the
//! shared clock detector."
//!
//! The one thing that makes this work at all: autocorrelating the feature's *first difference*
//! (transition impulses), not the raw feature. An earlier, simpler attempt at an RTTY presence
//! signature (`rtty::characterize_rtty_signature`) computed a flip-rate statistic directly on
//! the raw per-bit MARK/SPACE sign and found it did NOT separate real RTTY from noise -- data
//! bits are themselves close to a coin flip at 1x-per-bit granularity, so the raw signal's
//! autocorrelation is dominated by data-dependent character/word rhythm, not the underlying bit
//! clock. Differencing first collapses each transition down to a single impulse regardless of
//! how many hops of "no change" precede it, so autocorrelating the difference isolates the
//! actual symbol-clock periodicity instead of the (mode- and message-dependent) rhythm on top of
//! it. See `probe_dominant_symbol_period_separates_rtty_cw_and_noise` for real measured numbers.
//!
//! # Early promotion: don't always wait for the full buffer
//!
//! [`dominant_symbol_period`] takes whatever `feature` slice it's given, so nothing stops a
//! caller from checking confidence on a short prefix instead of a full multi-second buffer.
//! `probe_confidence_vs_duration_for_early_decoder_promotion` measures exactly that (RTTY, growing
//! prefixes of the same audio from 0.25s to 30s, at 20dB and 0dB SNR, against a matched-duration
//! noise floor across 10 seeds each time) and finds a real, usable asymmetry:
//!
//! - A strong (20dB) signal locks onto the correct baud with real separation from the noise
//!   floor by **1 second** (confidence ~5.5 vs. a 10-seed noise-floor max of ~3.9 at that same
//!   duration) -- there's no need to make a strong signal wait for a 30-second buffer just
//!   because a weak one needs it.
//! - Below 1 second the noise floor itself is untrustworthy: at 0.25s ten different noise seeds
//!   produced a max confidence of 4.39, higher than the real 20dB signal's own 3.25 at that same
//!   duration (which also recovered the WRONG baud). Any promotion policy MUST refuse to look at
//!   confidence before at least ~1 second of audio has accumulated, full stop -- there is no
//!   confidence-value fix for "not enough data yet."
//! - A marginal (0dB) signal does NOT lock early: it hovers at or below the noise floor from
//!   0.25s through roughly 3s and only pulls durably ahead of it somewhere around 5-10s. This is
//!   the expected trade, not a bug to chase -- a weak signal needs the averaging a short check
//!   can't give it, and the fast path only ever helps the case that doesn't need to wait anyway.
//!
//! A caller wiring this into a real pipeline should therefore: (1) never consult confidence
//! before a minimum duration (>=1s for the specific RTTY window/hop geometry measured here --
//! re-measure for other configurations, don't assume it carries over), (2) require the
//! confidence to clear a fixed bar on more than one consecutive check before promoting -- a
//! single check close in time to the noise floor's own spiky short-duration behavior is exactly
//! the false-positive risk the 0.25s/0.5s numbers above describe, and (3) keep accumulating and
//! re-checking up to whatever longer cap the pipeline already uses for the weak-signal case,
//! rather than giving up early just because the fast path didn't fire. None of this
//! early-promotion policy is implemented as code here (it's a caller/pipeline-level arbitration
//! decision, not a property of the primitive itself) -- see `digital_decoder.rs`'s own
//! `RTTY_LOCK_PERSISTENCE`-style arbitration in `hams_com` for the existing pattern this slots
//! into (as corroboration alongside its own character-count gate, not a replacement for it --
//! that gate's own false-alarm rate, 0.0127 false chars/decoder/s, is a much stronger presence
//! signal than this module's symbol-clock check on its own; corroboration only ever shortens an
//! ALREADY-passing lock from 2 windows to 1, it never substitutes for the character-count gate).
//!
//! IMPORTANT: "the same fixed bar" is NOT one number that travels safely between sample rates or
//! durations. `probe_corroboration_gate_at_bank_sample_rate` measures [`two_tone_window_len`]/
//! [`two_tone_hop_len`]'s own RTTY geometry at the RTTY bank's real 12kHz (not this module's own
//! 8kHz test rate) and finds a noticeably higher single-candidate, 1-second noise ceiling there
//! (max 4.79 over 20 seeds, vs. 8kHz's 3.87) -- a threshold picked from 8kHz measurements and
//! reused at 12kHz without re-checking would sit BELOW the real noise floor at the rate that
//! actually matters. Always calibrate against the exact sample rate, duration, and (single- vs.
//! multi-candidate) usage pattern the threshold will run against in production, not against
//! whatever rate was convenient to test at.

/// One measured symbol-clock hypothesis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SymbolClock {
    /// The best-fit symbol rate, in baud (symbols/second).
    pub baud: f64,
    /// How many times the winning lag's |autocorrelation| exceeds the mean |autocorrelation|
    /// across the whole scanned lag range -- not an absolute probability, and only meaningful
    /// relative to a noise-floor measurement taken the same way (same feature extractor, same
    /// lag range).
    ///
    /// This is "peak / mean," not a z-score ("(peak - mean) / std") -- that was tried first and
    /// measured WORSE at separating real RTTY from noise: RTTY's autocorrelation isn't a single
    /// clean spike against a flat floor, it has real (if smaller) correlation at neighboring
    /// sub-harmonic lags too (English text's own character/word rhythm bleeding into the
    /// profile), which inflates the z-score's mean AND std together and shrinks the apparent
    /// peak right along with the noise it's supposed to be measured against. Peak/mean is less
    /// sensitive to that because it only divides by the mean, not by a spread that the same
    /// contamination also inflates. Measured directly in
    /// `probe_dominant_symbol_period_separates_rtty_cw_and_noise`: z-score gave RTTY 3.22 vs. a
    /// 20-seed noise floor of mean 2.66/max 3.34 (overlapping -- not usable), while peak/mean on
    /// the same data gave RTTY 5.13 vs. noise mean 3.00/max 3.92 (clean separation), and CW
    /// 12.18 vs. noise mean 2.88/max 3.76 (also clean). Re-measure before changing this back.
    pub confidence: f64,
}

/// A lower bound on the scanned lag, regardless of what `max_baud` would otherwise allow.
/// First-differencing a white-noise-like feature induces a strong NEGATIVE autocorrelation at
/// lag 1 by construction (`d_k = x_k - x_{k-1}` makes adjacent differences anti-correlated), so
/// the lowest few lags carry a built-in artifact that has nothing to do with any real periodic
/// clock. Scanning from lag 1 lets that artifact dominate the noise floor and makes real signals
/// look weaker than they are by comparison -- measured directly: before this floor was added,
/// pure noise's winning lag against an RTTY-shaped search range landed at lag 3, right against
/// the search range's own minimum.
const MIN_LAG_FLOOR: usize = 4;

/// The signed (not absolute) autocorrelation of `feature`'s first difference, at every lag
/// corresponding to a baud rate in `[min_baud, max_baud]`. Returned as `(baud, signed_score)`
/// pairs, ordered from the highest baud (shortest lag) to the lowest. Exposed alongside
/// [`dominant_symbol_period`] (which is built on top of this) because the SIGN is itself useful
/// diagnostic evidence: a feature built from a symmetric bipolar quantity (e.g. a MARK/SPACE
/// ratio that spends roughly equal time on each side of zero) should show a clean NEGATIVE
/// trough at the true symbol lag and near-zero elsewhere, with no positive harmonic peaks --
/// confirming the underlying mechanism rather than just trusting a magnitude threshold. See
/// `probe_dominant_symbol_period_separates_rtty_cw_and_noise` for a real measurement of this.
///
/// `None` under the same degenerate conditions as `dominant_symbol_period`.
pub fn autocorrelation_profile(
    feature: &[f64],
    feature_rate_hz: f64,
    min_baud: f64,
    max_baud: f64,
) -> Option<Vec<(f64, f64)>> {
    if feature_rate_hz <= 0.0 || min_baud <= 0.0 || max_baud <= min_baud {
        return None;
    }
    if feature.len() < 4 {
        return None;
    }

    let diff: Vec<f64> = feature.windows(2).map(|w| w[1] - w[0]).collect();

    let min_lag = ((feature_rate_hz / max_baud).floor() as usize)
        .max(1)
        .max(MIN_LAG_FLOOR);
    let max_lag = (feature_rate_hz / min_baud).ceil() as usize;
    if min_lag >= max_lag || max_lag >= diff.len() {
        return None;
    }

    let mut profile = Vec::with_capacity(max_lag - min_lag + 1);
    for lag in min_lag..=max_lag {
        let n = diff.len() - lag;
        let sum: f64 = (0..n).map(|i| diff[i] * diff[i + lag]).sum();
        profile.push((feature_rate_hz / lag as f64, sum / n as f64));
    }
    Some(profile)
}

/// Finds a periodic symbol clock in `feature` (a scalar time series sampled at
/// `feature_rate_hz`) by autocorrelating its first difference over lags corresponding to baud
/// rates in `[min_baud, max_baud]` -- see [`autocorrelation_profile`], which this is built on.
/// `None` when `feature` is too short to cover even one period at `min_baud`, or the inputs are
/// degenerate (empty range, non-positive rate).
///
/// See the module doc comment for why the first difference, not the raw feature, is what gets
/// autocorrelated.
// [@ANCHOR: dominant_symbol_period]
pub fn dominant_symbol_period(
    feature: &[f64],
    feature_rate_hz: f64,
    min_baud: f64,
    max_baud: f64,
) -> Option<SymbolClock> {
    let profile = autocorrelation_profile(feature, feature_rate_hz, min_baud, max_baud)?;

    let abs_scores: Vec<f64> = profile.iter().map(|(_, s)| s.abs()).collect();
    let mean_score = abs_scores.iter().sum::<f64>() / abs_scores.len() as f64;
    if mean_score <= 0.0 {
        return None;
    }

    let (best_idx, &best_abs) = abs_scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;

    Some(SymbolClock {
        baud: profile[best_idx].0,
        confidence: best_abs / mean_score,
    })
}

/// A two-tone energy-ratio feature: for each `hop_len`-sample step, correlate a `window_len`-
/// sample window against `tone_a_hz` and `tone_b_hz` and report the signed ratio in dB (positive
/// = closer to `tone_a_hz`). The same per-window correlator RTTY's own MARK/SPACE detector uses
/// internally, but standalone and public so any two-tone-ish FSK signal can feed
/// [`dominant_symbol_period`] -- not just RTTY's own decoder.
///
/// `window_len` and `hop_len` are deliberately separate: `window_len` must be long enough to
/// actually separate `tone_a_hz` from `tone_b_hz` (roughly `sample_rate / (tone_b_hz -
/// tone_a_hz)` at minimum, comfortably a full symbol period), while `hop_len` -- the stride
/// between successive (overlapping) windows -- controls how finely transition edges are
/// resolved and should be well UNDER the suspected symbol period (a `window_len / 8` to
/// `window_len / 16` stride is a reasonable starting point). Conflating the two (one early
/// version of this function did) silently starves the tone correlator of enough samples to tell
/// the tones apart, which still finds the right baud but at a fraction of the true separation
/// between signal and noise -- see the module doc comment.
// [@ANCHOR: two_tone_ratio_feature]
pub fn two_tone_ratio_feature(
    samples: &[i16],
    tone_a_hz: f64,
    tone_b_hz: f64,
    sample_rate: u32,
    window_len: usize,
    hop_len: usize,
) -> Vec<f64> {
    if window_len == 0 || hop_len == 0 || sample_rate == 0 {
        return Vec::new();
    }

    let mut cos_a = Vec::with_capacity(window_len);
    let mut sin_a = Vec::with_capacity(window_len);
    let mut cos_b = Vec::with_capacity(window_len);
    let mut sin_b = Vec::with_capacity(window_len);
    for k in 0..window_len {
        let phase_a = 2.0 * std::f64::consts::PI * tone_a_hz * k as f64 / sample_rate as f64;
        let phase_b = 2.0 * std::f64::consts::PI * tone_b_hz * k as f64 / sample_rate as f64;
        cos_a.push(phase_a.cos());
        sin_a.push(phase_a.sin());
        cos_b.push(phase_b.cos());
        sin_b.push(phase_b.sin());
    }

    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + window_len <= samples.len() {
        let mut a_i = 0.0f64;
        let mut a_q = 0.0f64;
        let mut b_i = 0.0f64;
        let mut b_q = 0.0f64;
        for k in 0..window_len {
            let x = samples[pos + k] as f64 / i16::MAX as f64;
            a_i += x * cos_a[k];
            a_q += x * sin_a[k];
            b_i += x * cos_b[k];
            b_q += x * sin_b[k];
        }
        let a_energy = a_i * a_i + a_q * a_q;
        let b_energy = b_i * b_i + b_q * b_q;
        out.push(10.0 * (a_energy.max(1e-15) / b_energy.max(1e-15)).log10());
        pos += hop_len;
    }
    out
}

/// A short-frame RMS envelope feature, hopped every `frame_len` samples -- for on/off-keyed
/// signals (CW, Hellschreiber) rather than two-tone FSK ones. `frame_len` should be well under
/// the shortest real keyed element (a CW dit, a Hellschreiber dot) so the envelope resolves
/// individual on/off transitions instead of smearing several together into one frame.
// [@ANCHOR: envelope_feature]
pub fn envelope_feature(samples: &[i16], frame_len: usize) -> Vec<f64> {
    if frame_len == 0 {
        return Vec::new();
    }
    samples
        .chunks_exact(frame_len)
        .map(|chunk| {
            (chunk.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / frame_len as f64).sqrt()
        })
        .collect()
}

/// Chooses a `window_len` for [`two_tone_ratio_feature`] that satisfies both geometry
/// constraints a caller needs when feeding its output to [`dominant_symbol_period`] over a baud
/// search range with the given `max_baud`: long enough to actually separate two tones spaced
/// `shift_hz` apart (`window_len >= sample_rate / shift_hz`), but short enough that the sliding
/// window's own autocorrelation artifact (`sample_rate / window_len`, see `SymbolClock`'s own
/// doc comment for the mechanism -- a real, previously-shipped bug in an earlier version of this
/// module) falls OUTSIDE the search range (`window_len < sample_rate / max_baud`) instead of
/// landing on top of a real answer and being indistinguishable from one.
///
/// Picks the midpoint of the valid band. Returns `None` when the two constraints leave no valid
/// window at all (e.g. too narrow a shift for too high a baud at this sample rate) -- callers
/// must treat that as "this configuration can't be characterized this way," not force a bad
/// window through anyway.
pub fn two_tone_window_len(sample_rate: u32, shift_hz: f64, max_baud: f64) -> Option<usize> {
    let min_window = (sample_rate as f64 / shift_hz).ceil() as usize;
    let max_window_exclusive = (sample_rate as f64 / max_baud).floor() as usize;
    if max_window_exclusive <= min_window {
        return None;
    }
    Some((min_window + max_window_exclusive - 1) / 2)
}

/// A stride between overlapping [`two_tone_ratio_feature`] windows, given a `window_len` from
/// [`two_tone_window_len`]: well under the window length so transition edges are actually
/// resolved, while staying far enough from `window_len` itself that its own geometry artifact
/// (see that function's doc comment) lands outside the searched baud range too.
pub fn two_tone_hop_len(window_len: usize) -> usize {
    (window_len / 8).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtty::rtty_modulate;

    /// Uniform f64 in (0, 1), excluding 0 (needed so Box-Muller's ln() never sees 0). Same
    /// generator convention as `rtty.rs`'s own noise tests -- a small xorshift PRNG, not
    /// `rand`, to keep this crate's test dependencies minimal.
    fn next_uniform(state: &mut u64) -> f64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        ((*state >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
    }

    /// Real Gaussian noise (Box-Muller), not periodic synthetic filler -- see this crate's own
    /// processing-gain harness for why a repeated short "noise" buffer or a periodic synthetic
    /// waveform silently breaks any test that measures periodicity.
    fn gaussian_noise(n: usize, rms: f64, seed: u64) -> Vec<i16> {
        let mut state = seed.max(1);
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let u1 = next_uniform(&mut state);
            let u2 = next_uniform(&mut state);
            let mag = (-2.0 * u1.ln()).sqrt();
            let z0 = mag * (2.0 * std::f64::consts::PI * u2).cos();
            let z1 = mag * (2.0 * std::f64::consts::PI * u2).sin();
            for z in [z0, z1] {
                if out.len() >= n {
                    break;
                }
                out.push((z * rms).clamp(i16::MIN as f64, i16::MAX as f64) as i16);
            }
        }
        out
    }

    /// Adds real Gaussian noise to `clean` at a target SNR in dB, scaling the noise RMS off
    /// `clean`'s own measured RMS -- the same approach as `tests/rtty_processing_gain_
    /// harness.rs`'s `add_gaussian_noise_at_snr` (duplicated here in miniature rather than
    /// shared, since that lives in a separate integration-test binary with no library path back
    /// into this crate's own unit tests).
    fn add_gaussian_noise_at_snr(clean: &[i16], snr_db: f64, seed: u64) -> Vec<i16> {
        let signal_rms =
            (clean.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / clean.len() as f64).sqrt();
        let noise_rms = signal_rms / 10f64.powf(snr_db / 20.0);
        let noise = gaussian_noise(clean.len(), noise_rms, seed);
        clean
            .iter()
            .zip(noise.iter())
            .map(|(&s, &n)| (s as i32 + n as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
            .collect()
    }

    /// A real on/off-keyed tone at `tone_hz`, Morse "PARIS" (the standard WPM calibration word)
    /// repeated to fill `seconds`, keyed at `dit_seconds` per dit unit. Each "on" period is a
    /// real sine wave, not a synthetic amplitude gate on silence.
    fn synth_cw(tone_hz: f64, dit_seconds: f64, seconds: f64, sample_rate: u32) -> Vec<i16> {
        // P(.--.) A(.-) R(.-.) I(..) S(...), dit units, on/off pairs including inter-element
        // (1), inter-letter (3), and word (7) gaps.
        let pattern_units: &[(bool, f64)] = &[
            (true, 1.0),
            (false, 1.0),
            (true, 3.0),
            (false, 1.0),
            (true, 3.0),
            (false, 1.0),
            (true, 1.0),
            (false, 3.0),
            (true, 1.0),
            (false, 1.0),
            (true, 3.0),
            (false, 3.0),
            (true, 1.0),
            (false, 1.0),
            (true, 3.0),
            (false, 1.0),
            (true, 1.0),
            (false, 3.0),
            (true, 1.0),
            (false, 1.0),
            (true, 1.0),
            (false, 1.0),
            (true, 1.0),
            (false, 7.0),
        ];
        let mut samples = Vec::new();
        let mut phase = 0.0f64;
        let phase_inc = 2.0 * std::f64::consts::PI * tone_hz / sample_rate as f64;
        'outer: loop {
            for &(on, units) in pattern_units {
                let n = (units * dit_seconds * sample_rate as f64) as usize;
                for _ in 0..n {
                    if samples.len() as f64 / sample_rate as f64 >= seconds {
                        break 'outer;
                    }
                    samples.push(if on {
                        (phase.sin() * (i16::MAX as f64 * 0.8)) as i16
                    } else {
                        0
                    });
                    phase += phase_inc;
                }
            }
        }
        samples
    }

    const SAMPLE_RATE: u32 = 8000;
    const RTTY_MARK_HZ: f64 = 2125.0;
    const RTTY_SHIFT_HZ: f64 = 170.0;
    const RTTY_BAUD: f64 = 45.45;
    const CW_DIT_SECONDS: f64 = 0.06; // 20 WPM

    /// A sliding (overlapping) correlator window has its OWN artifact: averaging `W =
    /// window_len/hop_len` feature samples together gives the feature a triangular
    /// autocorrelation of width `W`, and first-differencing that produces a negative trough at
    /// exactly lag `W` -- i.e. at baud `feature_rate_hz / W = sample_rate / window_len` (hop
    /// cancels out of that ratio). Deriving `window_len` from the very symbol period being
    /// searched for (an earlier version of this function used one full bit period, 176 samples
    /// at 8kHz) guarantees that artifact lands exactly on the answer under test, which is
    /// genuinely indistinguishable from a real clock by every statistic tried here -- caught
    /// only by noticing pure Gaussian noise "recovered" the identical 45.45 baud.
    ///
    /// The fix (now [`two_tone_window_len`], the shared library function this delegates to) picks
    /// `window_len` to satisfy two constraints at once, as a function of `sample_rate` (an
    /// earlier version of this test helper hardcoded a number measured at one sample rate and
    /// silently ignored its own `sample_rate` argument -- it would have quietly under-resolved
    /// the tone separation again the moment this ran at the RTTY bank's real 12kHz instead of
    /// this test module's 8kHz): long enough to separate MARK from SPACE (`window_len >=
    /// sample_rate / shift_hz`), but short enough that its own geometry artifact (`sample_rate /
    /// window_len`) falls OUTSIDE the baud range being searched (`window_len < sample_rate /
    /// max_baud`). Panics if the two constraints don't leave a valid window, which is a real
    /// configuration error (e.g. too narrow a shift for too high a baud) this test suite should
    /// never hit, not something to silently paper over.
    fn rtty_window_len(sample_rate: u32) -> usize {
        two_tone_window_len(sample_rate, RTTY_SHIFT_HZ, 100.0)
            .unwrap_or_else(|| panic!("no valid RTTY correlator window at {sample_rate}Hz"))
    }

    /// Stride between (overlapping) windows -- see [`two_tone_hop_len`], the shared library
    /// function this delegates to.
    fn rtty_hop_len(sample_rate: u32) -> usize {
        two_tone_hop_len(rtty_window_len(sample_rate))
    }

    /// ~2.5ms frames: short enough to resolve a 60ms dit cleanly.
    fn cw_frame_len(sample_rate: u32) -> usize {
        (sample_rate as f64 * 0.0025).round() as usize
    }

    /// Enough repeats of a pangram-ish sentence to yield at least `min_seconds` of real RTTY
    /// audio at `RTTY_BAUD` -- long enough for autocorrelation's variance to shrink to something
    /// usable, not just a couple of words.
    fn rtty_audio_at_least(min_seconds: f64, sample_rate: u32) -> Vec<i16> {
        let phrase = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG 0123456789 ";
        let mut text = String::new();
        // ~0.165s/char at RTTY_BAUD; pad with an extra repeat so rounding never falls short.
        let repeats = ((min_seconds / (phrase.len() as f64 * 0.165)).ceil() as usize) + 1;
        for _ in 0..repeats {
            text.push_str(phrase);
        }
        rtty_modulate(&text, RTTY_MARK_HZ, sample_rate)
    }

    #[test]
    // Tests [@ANCHOR: dominant_symbol_period]
    fn rejects_degenerate_inputs_instead_of_panicking() {
        assert!(dominant_symbol_period(&[], 1000.0, 10.0, 100.0).is_none());
        assert!(dominant_symbol_period(&[1.0, 2.0, 3.0], 1000.0, 10.0, 100.0).is_none());
        assert!(dominant_symbol_period(&[1.0, 2.0, 3.0, 4.0], 0.0, 10.0, 100.0).is_none());
        assert!(dominant_symbol_period(&[1.0, 2.0, 3.0, 4.0], 1000.0, 100.0, 10.0).is_none());
        assert!(two_tone_ratio_feature(&[1, 2, 3], 1000.0, 1200.0, 0, 10, 5).is_empty());
        assert!(envelope_feature(&[1, 2, 3], 0).is_empty());
    }

    #[test]
    // Tests [@ANCHOR: dominant_symbol_period]
    // Tests [@ANCHOR: two_tone_ratio_feature]
    fn recovers_real_rtty_baud_from_two_tone_ratio_feature() {
        // Both this test module's own 8kHz and the real RTTY auto-tune bank's deployment rate
        // (`RTTY_BANK_SAMPLE_RATE = 12000` in `hams_com`'s `digital_decoder.rs`) -- an earlier
        // version of `rtty_window_len` hardcoded a window measured only at 8kHz and silently
        // ignored its own `sample_rate` argument, which would have quietly under-resolved MARK/
        // SPACE separation the moment this ran at 12kHz instead. See that function's own doc
        // comment for the two-constraint geometry this is actually checking.
        for sample_rate in [8000u32, 12000u32] {
            let audio = rtty_audio_at_least(30.0, sample_rate);
            let window_len = rtty_window_len(sample_rate);
            let hop_len = rtty_hop_len(sample_rate);
            let feature = two_tone_ratio_feature(
                &audio,
                RTTY_MARK_HZ,
                RTTY_MARK_HZ + RTTY_SHIFT_HZ,
                sample_rate,
                window_len,
                hop_len,
            );
            let feature_rate_hz = sample_rate as f64 / hop_len as f64;

            let clock = dominant_symbol_period(&feature, feature_rate_hz, 20.0, 100.0)
                .unwrap_or_else(|| {
                    panic!("a real RTTY signal at {sample_rate}Hz must yield a symbol clock")
                });

            assert!(
                (clock.baud - RTTY_BAUD).abs() < 3.0,
                "at {sample_rate}Hz: expected ~{RTTY_BAUD} baud, got {}",
                clock.baud
            );
            // The exact confidence floor is measured in the ignored diagnostic probe below (run
            // against a held-out noise seed, not one of its calibration seeds); here we only
            // assert it clears a conservative bar so this test stays a real regression check
            // rather than pinning an exact number that would need updating on every unrelated
            // tweak. Measured (probe, 8kHz): real RTTY ~5.1, a 20-seed noise floor of mean
            // 3.0/max 3.9. 4.5 sits between them with margin on both sides.
            assert!(
                clock.confidence > 4.5,
                "at {sample_rate}Hz: expected a clearly-above-floor confidence, got {}",
                clock.confidence
            );
        }
    }

    #[test]
    // Tests [@ANCHOR: dominant_symbol_period]
    // Tests [@ANCHOR: envelope_feature]
    fn recovers_real_cw_dit_rate_from_envelope_feature() {
        let dit_rate = 1.0 / CW_DIT_SECONDS;
        let audio = synth_cw(700.0, CW_DIT_SECONDS, 24.0, SAMPLE_RATE);
        let frame_len = cw_frame_len(SAMPLE_RATE);
        let feature = envelope_feature(&audio, frame_len);
        let feature_rate_hz = SAMPLE_RATE as f64 / frame_len as f64;

        let clock =
            dominant_symbol_period(&feature, feature_rate_hz, dit_rate / 2.0, dit_rate * 2.0)
                .expect("a real CW signal must yield a symbol clock");

        assert!(
            (clock.baud - dit_rate).abs() < dit_rate * 0.5,
            "expected somewhere near the {dit_rate:.2}Hz dit rate, got {}",
            clock.baud
        );
        // Measured (probe): real CW ~12.2 (machine-exact keying, see the probe's own note that
        // this is optimistic vs. hand-sent CW), a 20-seed noise floor of mean 2.9/max 3.8.
        assert!(
            clock.confidence > 6.0,
            "expected an above-floor confidence, got {}",
            clock.confidence
        );
    }

    #[test]
    // Tests [@ANCHOR: dominant_symbol_period]
    fn noise_does_not_produce_a_confident_symbol_clock() {
        // A seed deliberately outside the diagnostic probe's own 1..=20 calibration range below
        // -- asserting against a seed used to pick the threshold would just be fitting to the
        // calibration set, not testing anything.
        let held_out_seed = 909_090_909u64;
        let window_len = rtty_window_len(SAMPLE_RATE);
        let hop_len = rtty_hop_len(SAMPLE_RATE);
        let frame_len = cw_frame_len(SAMPLE_RATE);
        let dit_rate = 1.0 / CW_DIT_SECONDS;

        let noise = gaussian_noise(SAMPLE_RATE as usize * 30, 5000.0, held_out_seed);

        let rtty_like = two_tone_ratio_feature(
            &noise,
            RTTY_MARK_HZ,
            RTTY_MARK_HZ + RTTY_SHIFT_HZ,
            SAMPLE_RATE,
            window_len,
            hop_len,
        );
        let feature_rate_hz = SAMPLE_RATE as f64 / hop_len as f64;
        let clock = dominant_symbol_period(&rtty_like, feature_rate_hz, 20.0, 100.0)
            .expect("even noise yields a best-fit lag, just not a confident one");
        assert!(
            clock.confidence < 4.5,
            "noise should not clear the same confidence bar real RTTY does, got {}",
            clock.confidence
        );
        // A cheap regression guard for the specific bug this module was built around: if
        // `rtty_window_len`/`rtty_hop_len` ever again derive the window from the RTTY baud
        // itself, pure noise's own geometry artifact lands exactly on RTTY_BAUD and this would
        // start failing here well before anyone had to notice it by hand.
        assert!(
            (clock.baud - RTTY_BAUD).abs() > 2.0,
            "noise's winning baud ({}) landed suspiciously close to RTTY_BAUD ({RTTY_BAUD}) -- \
             this is the exact signature of the window-geometry artifact this module's own \
             doc comments warn about, not a real coincidence",
            clock.baud
        );

        let cw_like = envelope_feature(&noise, frame_len);
        let feature_rate_hz = SAMPLE_RATE as f64 / frame_len as f64;
        let clock =
            dominant_symbol_period(&cw_like, feature_rate_hz, dit_rate / 2.0, dit_rate * 2.0)
                .expect("even noise yields a best-fit lag, just not a confident one");
        assert!(
            clock.confidence < 4.5,
            "noise should not clear the same confidence bar real CW does, got {}",
            clock.confidence
        );
    }

    /// The real evidence behind this module's own doc comments: prints measured confidence for
    /// RTTY, CW, and a 20-seed noise floor side by side, plus the signed autocorrelation profile
    /// around the RTTY winner -- confirming it's a clean negative trough (the expected shape for
    /// a symmetric bipolar MARK/SPACE feature) rather than an artifact.
    ///
    /// Calibrates thresholds from seeds 1..=20 only; `noise_does_not_produce_a_confident_
    /// symbol_clock` asserts against a seed well outside that range so the regression test isn't
    /// just fit to its own calibration data.
    ///
    /// Note the CW number here is optimistic relative to real on-air hand-sent CW: `synth_cw`
    /// keys perfectly machine-exact dit/dah/gap ratios with zero timing jitter. Real hand keying
    /// jitters enough to broaden this peak; treat this measurement as an upper bound, not a
    /// field expectation.
    ///
    /// Also sweeps RTTY confidence against real mixed-in noise at a range of SNRs (not just
    /// signal-alone vs. noise-alone, which says nothing about the case this actually has to
    /// handle). Measured result, genuinely better than expected: confidence stays in the 4.3-5.9
    /// range all the way from +20dB down to -20dB SNR, degrading gracefully rather than
    /// collapsing at some cliff -- comfortably above the noise floor's own ~4.1 max at every
    /// point except a brief dip to 4.34 at -10dB. This symbol-clock detector isn't trying to
    /// demodulate bits, just detect a periodic energy-ratio wobble averaged over the WHOLE
    /// buffer (30s here), so it has far more processing gain available to it than a real
    /// character-by-character decoder would at the same SNR -- it staying usable this far into
    /// the noise is real, not a measurement artifact, but don't extrapolate it to the RTTY
    /// decoder's own much harder job of getting individual bits right.
    ///
    /// `cargo test -p ham_digital_modes probe_dominant_symbol_period -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_dominant_symbol_period_separates_rtty_cw_and_noise() {
        let window_len = rtty_window_len(SAMPLE_RATE);
        let hop_len = rtty_hop_len(SAMPLE_RATE);
        let rtty_feature_rate_hz = SAMPLE_RATE as f64 / hop_len as f64;

        let rtty_audio = rtty_audio_at_least(30.0, SAMPLE_RATE);
        let rtty_feature = two_tone_ratio_feature(
            &rtty_audio,
            RTTY_MARK_HZ,
            RTTY_MARK_HZ + RTTY_SHIFT_HZ,
            SAMPLE_RATE,
            window_len,
            hop_len,
        );
        let rtty_clock =
            dominant_symbol_period(&rtty_feature, rtty_feature_rate_hz, 20.0, 100.0).unwrap();

        let dit_rate = 1.0 / CW_DIT_SECONDS;
        let cw_frame = cw_frame_len(SAMPLE_RATE);
        let cw_feature_rate_hz = SAMPLE_RATE as f64 / cw_frame as f64;
        let cw_audio = synth_cw(700.0, CW_DIT_SECONDS, 24.0, SAMPLE_RATE);
        let cw_feature = envelope_feature(&cw_audio, cw_frame);
        let cw_clock = dominant_symbol_period(
            &cw_feature,
            cw_feature_rate_hz,
            dit_rate / 2.0,
            dit_rate * 2.0,
        )
        .unwrap();

        println!(
            "RTTY:  baud={:.2} (true {RTTY_BAUD}) confidence={:.2}",
            rtty_clock.baud, rtty_clock.confidence
        );
        println!(
            "CW:    baud={:.2} (true dit rate {dit_rate:.2}) confidence={:.2}  [optimistic: zero keying jitter]",
            cw_clock.baud, cw_clock.confidence
        );

        // The signed profile around the RTTY winner: expect one clean negative trough at the
        // true symbol lag and nothing but noise elsewhere -- no positive harmonic peaks.
        let profile =
            autocorrelation_profile(&rtty_feature, rtty_feature_rate_hz, 20.0, 100.0).unwrap();
        println!("RTTY signed autocorrelation profile (baud, signed_score):");
        for (baud, score) in &profile {
            println!("  {baud:7.2} baud: {score:+.6}");
        }

        // Noise floor across 20 independent seeds, not just one -- a single noise realization is
        // itself a noisy estimate of "what does noise do here." Matches the RTTY/CW audio
        // duration above (30s / 24s) rather than a shorter buffer, so the floor isn't measured
        // with less averaging than the signal it's compared against.
        let mut rtty_confidences = Vec::new();
        let mut cw_confidences = Vec::new();
        for seed in 1u64..=20 {
            let noise = gaussian_noise(SAMPLE_RATE as usize * 30, 5000.0, seed * 7919);
            let noise_rtty_feature = two_tone_ratio_feature(
                &noise,
                RTTY_MARK_HZ,
                RTTY_MARK_HZ + RTTY_SHIFT_HZ,
                SAMPLE_RATE,
                window_len,
                hop_len,
            );
            if let Some(c) =
                dominant_symbol_period(&noise_rtty_feature, rtty_feature_rate_hz, 20.0, 100.0)
            {
                if seed == 1 {
                    println!(
                        "Noise (RTTY feature) seed 1 winner: baud={:.2} confidence={:.2} \
                         (compare against RTTY's own {RTTY_BAUD} -- if these coincide, the \
                         window geometry itself is the artifact, not a real clock)",
                        c.baud, c.confidence
                    );
                }
                rtty_confidences.push(c.confidence);
            }
            // CW's own signal above is 24s, not 30s -- reuse a 24s-truncated slice of the same
            // noise buffer rather than the full 30s so the floor isn't measured with MORE
            // averaging than the signal it's being compared against (that would flatter the
            // separation, not just measure it).
            let cw_noise_len = SAMPLE_RATE as usize * 24;
            let noise_cw_feature = envelope_feature(&noise[..cw_noise_len], cw_frame);
            if let Some(c) = dominant_symbol_period(
                &noise_cw_feature,
                cw_feature_rate_hz,
                dit_rate / 2.0,
                dit_rate * 2.0,
            ) {
                cw_confidences.push(c.confidence);
            }
        }
        let max_of = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max);
        let mean_of = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        println!(
            "Noise (RTTY feature) over 20 seeds: mean={:.2} max={:.2}",
            mean_of(&rtty_confidences),
            max_of(&rtty_confidences)
        );
        println!(
            "Noise (CW feature) over 20 seeds:   mean={:.2} max={:.2}",
            mean_of(&cw_confidences),
            max_of(&cw_confidences)
        );

        // Everything above is signal-ALONE or noise-ALONE. A real front-end classifier only
        // ever sees audio with noise already in it, so the number that actually decides whether
        // this is usable is confidence at real SNRs, not on pristine synthesized audio -- an
        // RTTY confidence of 5.1 against a noise-floor max of 3.9 (0.63 measured separation gap,
        // see `SymbolClock::confidence`'s doc comment) doesn't say anything about whether a real
        // received signal clears the gate before noise starts eating the peak.
        println!("RTTY confidence vs. SNR (mixed real signal + real noise, not separate):");
        for snr_db in [20.0, 10.0, 5.0, 0.0, -3.0, -6.0, -10.0, -15.0, -20.0] {
            let noisy = add_gaussian_noise_at_snr(&rtty_audio, snr_db, 555_555);
            let feature = two_tone_ratio_feature(
                &noisy,
                RTTY_MARK_HZ,
                RTTY_MARK_HZ + RTTY_SHIFT_HZ,
                SAMPLE_RATE,
                window_len,
                hop_len,
            );
            match dominant_symbol_period(&feature, rtty_feature_rate_hz, 20.0, 100.0) {
                Some(c) => println!(
                    "  {snr_db:+5.1}dB SNR: baud={:.2} confidence={:.2}",
                    c.baud, c.confidence
                ),
                None => println!("  {snr_db:+5.1}dB SNR: no symbol clock found"),
            }
        }
    }

    /// How fast does confidence become trustworthy as a function of HOW MUCH audio has been
    /// seen so far -- the real question behind promoting a candidate to a real decoder as soon
    /// as it locks, rather than always waiting for a fixed long buffer. Sweeps confidence at
    /// growing prefixes of the same audio (0.25s up to 30s) for a strong (20dB) and a marginal
    /// (0dB) signal, against a matched-duration noise floor across several seeds at each
    /// duration (short buffers are a noisier floor estimate too, not just a noisier signal
    /// estimate -- has to be measured at the same durations, not assumed to inherit the 30s
    /// floor from the probe above).
    ///
    /// `cargo test -p ham_digital_modes probe_confidence_vs_duration -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_confidence_vs_duration_for_early_decoder_promotion() {
        let window_len = rtty_window_len(SAMPLE_RATE);
        let hop_len = rtty_hop_len(SAMPLE_RATE);
        let feature_rate_hz = SAMPLE_RATE as f64 / hop_len as f64;

        let clean_audio = rtty_audio_at_least(30.0, SAMPLE_RATE);
        let strong_audio = add_gaussian_noise_at_snr(&clean_audio, 20.0, 42);
        let marginal_audio = add_gaussian_noise_at_snr(&clean_audio, 0.0, 42);

        let durations = [0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 20.0, 30.0];
        let confidence_at = |audio: &[i16], seconds: f64| -> Option<SymbolClock> {
            let n = ((seconds * SAMPLE_RATE as f64) as usize).min(audio.len());
            let feature = two_tone_ratio_feature(
                &audio[..n],
                RTTY_MARK_HZ,
                RTTY_MARK_HZ + RTTY_SHIFT_HZ,
                SAMPLE_RATE,
                window_len,
                hop_len,
            );
            dominant_symbol_period(&feature, feature_rate_hz, 20.0, 100.0)
        };

        println!("duration(s)  strong(20dB)        marginal(0dB)       noise floor (10 seeds)");
        for &seconds in &durations {
            let strong = confidence_at(&strong_audio, seconds);
            let marginal = confidence_at(&marginal_audio, seconds);

            let mut noise_confidences = Vec::new();
            for seed in 1u64..=10 {
                let noise = gaussian_noise(
                    ((seconds * SAMPLE_RATE as f64) as usize).max(1),
                    5000.0,
                    seed * 104_729,
                );
                if let Some(c) = confidence_at(&noise, seconds) {
                    noise_confidences.push(c.confidence);
                }
            }
            let noise_max = noise_confidences.iter().cloned().fold(f64::MIN, f64::max);
            let noise_mean =
                noise_confidences.iter().sum::<f64>() / noise_confidences.len().max(1) as f64;

            println!(
                "{seconds:8.2}   {}   {}   mean={:.2} max={:.2}",
                strong
                    .map(|c| format!("baud={:6.2} conf={:5.2}", c.baud, c.confidence))
                    .unwrap_or_else(|| "  (none)          ".to_string()),
                marginal
                    .map(|c| format!("baud={:6.2} conf={:5.2}", c.baud, c.confidence))
                    .unwrap_or_else(|| "  (none)          ".to_string()),
                noise_mean,
                noise_max
            );
        }
    }

    /// The specific number the `digital_decoder.rs` RTTY-bank corroboration gate lives or dies
    /// on: does a 1-second, single-candidate confidence check clear `RTTY_CORROBORATION_
    /// CONFIDENCE` (4.5, chosen from THIS module's own 8kHz measurements) when run at the
    /// bank's REAL 12kHz rate instead? `probe_confidence_vs_duration_for_early_decoder_
    /// promotion` above never checked this -- it's hardcoded to `SAMPLE_RATE` (8kHz). Skipping
    /// this check would ship a corroboration path with no evidence it ever actually fires: the
    /// existing persistence-2 path still locks and still passes every RTTY-bank test either way,
    /// so a wrong threshold here fails silently, not loudly.
    ///
    /// `cargo test -p ham_digital_modes probe_corroboration_gate_at_bank_sample_rate -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_corroboration_gate_at_bank_sample_rate() {
        const BANK_SAMPLE_RATE: u32 = 12000;
        let window_len = two_tone_window_len(BANK_SAMPLE_RATE, RTTY_SHIFT_HZ, 100.0)
            .expect("valid window at the bank's real sample rate");
        let hop_len = two_tone_hop_len(window_len);
        let feature_rate_hz = BANK_SAMPLE_RATE as f64 / hop_len as f64;

        let confidence_at_1s = |audio: &[i16]| -> Option<SymbolClock> {
            let n = (BANK_SAMPLE_RATE as usize).min(audio.len());
            let feature = two_tone_ratio_feature(
                &audio[..n],
                RTTY_MARK_HZ,
                RTTY_MARK_HZ + RTTY_SHIFT_HZ,
                BANK_SAMPLE_RATE,
                window_len,
                hop_len,
            );
            dominant_symbol_period(&feature, feature_rate_hz, 20.0, 100.0)
        };

        let clean_audio = rtty_audio_at_least(2.0, BANK_SAMPLE_RATE);
        for snr_db in [20.0, 10.0, 5.0, 0.0] {
            let noisy = add_gaussian_noise_at_snr(&clean_audio, snr_db, 42);
            match confidence_at_1s(&noisy) {
                Some(c) => println!(
                    "{snr_db:+5.1}dB SNR @ 12kHz, 1s: baud={:.2} confidence={:.2} \
                     (clears digital_decoder.rs's RTTY_CORROBORATION_CONFIDENCE=5.5: {})",
                    c.baud,
                    c.confidence,
                    c.confidence > 5.5
                ),
                None => println!("{snr_db:+5.1}dB SNR @ 12kHz, 1s: no symbol clock found"),
            }
        }

        let mut noise_confidences = Vec::new();
        for seed in 1u64..=20 {
            let noise = gaussian_noise(BANK_SAMPLE_RATE as usize, 5000.0, seed * 104_729);
            if let Some(c) = confidence_at_1s(&noise) {
                noise_confidences.push(c.confidence);
            }
        }
        let mean = noise_confidences.iter().sum::<f64>() / noise_confidences.len() as f64;
        let max = noise_confidences.iter().cloned().fold(f64::MIN, f64::max);
        println!(
            "noise floor @ 12kHz, 1s, single candidate, 20 seeds: mean={mean:.2} max={max:.2}"
        );
    }

    /// The real cost of the `digital_decoder.rs` RTTY-bank corroboration check: one
    /// `two_tone_ratio_feature` + `dominant_symbol_period` call on 1 second of 12kHz audio
    /// (12,000 samples), run once per 1-second window on the single already-winning candidate --
    /// NOT per chunk, and NOT across the whole ~63-candidate bank (see `RTTY_CORROBORATION_
    /// CONFIDENCE`'s own doc comment in `digital_decoder.rs` for why only the winner is
    /// checked). Expressed as a fraction of the 1-second window it runs against, the same
    /// budget-fraction convention `probe_rtty_bank_scaling_cost` in `digital_decoder.rs` uses
    /// for the bank's own per-chunk cost, so the two numbers are directly comparable.
    ///
    /// Real, non-repeating Gaussian noise input (not a periodic buffer -- see this crate's own
    /// processing-gain harness for why that distinction matters for a correlator's timing), with
    /// a warm-up phase excluded from the measurement.
    ///
    /// `cargo test --release -p ham_digital_modes probe_corroboration_check_cpu_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_corroboration_check_cpu_cost() {
        const BANK_SAMPLE_RATE: u32 = 12000;
        let window_len = two_tone_window_len(BANK_SAMPLE_RATE, RTTY_SHIFT_HZ, 100.0)
            .expect("valid window at the bank's real sample rate");
        let hop_len = two_tone_hop_len(window_len);
        let feature_rate_hz = BANK_SAMPLE_RATE as f64 / hop_len as f64;

        // One long, non-repeating noise buffer sliced sequentially for warm-up and every timed
        // trial, matching `probe_rtty_bank_scaling_cost`'s own found-bug list in
        // `digital_decoder.rs` (a repeated short buffer is itself periodic and silently distorts
        // a correlator's measured cost).
        let n_trials = 200;
        let window_samples = BANK_SAMPLE_RATE as usize; // 1 second
        let long_noise = gaussian_noise(window_samples * (n_trials + 5), 5000.0, 0xC0FFEE);

        let run_once = |audio: &[i16]| {
            let feature = two_tone_ratio_feature(
                audio,
                RTTY_MARK_HZ,
                RTTY_MARK_HZ + RTTY_SHIFT_HZ,
                BANK_SAMPLE_RATE,
                window_len,
                hop_len,
            );
            std::hint::black_box(dominant_symbol_period(
                &feature,
                feature_rate_hz,
                20.0,
                100.0,
            ));
        };

        // Warm-up (5 calls on fresh, never-timed samples), then time n_trials fresh calls.
        for i in 0..5 {
            run_once(&long_noise[i * window_samples..(i + 1) * window_samples]);
        }
        let start = std::time::Instant::now();
        for i in 5..5 + n_trials {
            run_once(&long_noise[i * window_samples..(i + 1) * window_samples]);
        }
        let elapsed = start.elapsed();
        let per_call_us = elapsed.as_micros() as f64 / n_trials as f64;
        let window_duration_ms = 1000.0; // one 1-second window
        let budget_fraction = per_call_us / 1000.0 / window_duration_ms;

        println!(
            "corroboration check: {per_call_us:.1}us/call against a 1-second window \
             ({budget_fraction:.6}x that window's own real-time budget)"
        );
    }
}
