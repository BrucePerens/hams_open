// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point voiced/unvoiced decision -- `EncoderFixed`'s own real
//! production path. The original `f32` version (`is_voiced`/
//! `VoicingState`, the classic energy + zero-crossing-rate heuristic)
//! moved to `floating_reference::voicing` once this module's own
//! `is_voiced_fixed` became the only real caller; see that module's own
//! doc comment for the algorithm's own rationale, still accurate here.

/// Fixed-point candidate for `floating_reference::voicing::is_voiced`
/// (`FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s voicing.rs
/// stage). `is_voiced`'s own `energy_db = 10*log10(energy)` step turned
/// out to be a real dependency on `fixed_point.rs`'s `log2_lut` (whose
/// own interpolation is still genuine `f32`, a gap flagged separately in
/// this punch list) -- rather than build on top of that incomplete
/// piece, this sidesteps the log entirely: the dB-domain comparison
/// `energy_db > noise_floor_db + MARGIN_DB` is mathematically equivalent
/// to a **linear**-domain comparison `energy > noise_floor_energy *
/// 10^(MARGIN_DB/10)`, so the threshold check needs no runtime log or
/// pow at all, just one precomputed constant ratio.
///
/// **This is a real, deliberate algorithmic difference, not a
/// numerically-identical refactor**: tracking the noise floor as an EMA
/// over *linear* energy is not the same as an EMA over *dB* (a
/// log-domain EMA is closer to a geometric-mean-flavored average of the
/// underlying energies; a linear-domain EMA is arithmetic-flavored) --
/// permissible per this module's own doc comment ("doesn't need to
/// reproduce [the reference]'s exact decision, only make a reasonable
/// one"), but validated below against the *same* real test scenarios
/// `is_voiced` itself is validated against, not assumed equivalent.
#[derive(Default)]
pub struct VoicingStateFixed {
    /// Linear mean-squared-amplitude units (raw `i16` sample scale),
    /// not dB.
    noise_floor_energy: i64,
}

impl VoicingStateFixed {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Q16.16 for the linear margin ratio and the EMA update below.
const MARGIN_RATIO_FRAC_BITS: u32 = 16;
/// `round(10^(MARGIN_DB/10) * 2^16)` -- `MARGIN_DB`'s own real value
/// (12.0) baked in at compile time via a literal, not computed at
/// runtime (this candidate has no runtime `pow`/`log` at all). Real
/// value: `10^1.2 ~ 15.848931924611133`, quantized here to
/// `15.84893798828125` -- 4e-6 relative error, irrelevant next to the
/// voicing decision's own real design freedom.
const MARGIN_RATIO_Q16: i64 = 1_038_676;
/// `NOISE_BETA` (0.05) as an exact integer divisor for the EMA update
/// below (`1/20 == 0.05` exactly, unlike an arbitrary float constant --
/// no quantization error in the update rate itself).
const NOISE_BETA_DIVISOR: i64 = 20;

// [@ANCHOR: div_round_i64]
fn div_round_i64(n: i64, d: i64) -> i64 {
    debug_assert!(d > 0, "div_round_i64: divisor must be positive, got {d}");
    let half = d / 2;
    if n >= 0 {
        (n + half) / d
    } else {
        (n - half) / d
    }
}

/// Decides voicing from raw `i16` samples -- no `f32` anywhere in this
/// function. `zero_crossing_rate < ZCR_THRESH` (0.15 = 3/20) is checked
/// via cross-multiplication (`crossings*20 < 3*(n-1)`) instead of a
/// float division, exact for the real, small `n` this always runs with.
///
/// `samples` empty is a real, foreseeable input for this `pub fn` (the
/// crate's own sole caller, `EncoderFixed::encode`, always passes a
/// fixed non-empty `M_PITCH - N_SAMP`-length window, but nothing in
/// this function's own signature or doc comment enforces that on a
/// future or external caller) -- an empty slice has no energy to
/// measure and is judged unvoiced rather than panicking on the `energy
/// = energy_acc / n` integer division below (`n == 0`).
// [@ANCHOR: is_voiced_fixed]
pub fn is_voiced_fixed(state: &mut VoicingStateFixed, samples: &[i16]) -> bool {
    if samples.is_empty() {
        return false;
    }
    let n = samples.len() as i64;

    let mut crossings: i64 = 0;
    for w in samples.windows(2) {
        if (w[0] >= 0) != (w[1] >= 0) {
            crossings += 1;
        }
    }
    let zcr_ok = crossings * 20 < 3 * (n - 1);

    let mut energy_acc: i64 = 0;
    for &s in samples {
        energy_acc += (s as i64) * (s as i64);
    }
    let energy = energy_acc / n;

    let energy_scaled = energy.max(1) << MARGIN_RATIO_FRAC_BITS;
    let floor_scaled = state.noise_floor_energy.max(1) * MARGIN_RATIO_Q16;
    let energy_ok = energy_scaled > floor_scaled;

    let voiced = energy_ok && zcr_ok;

    if !voiced {
        let diff = energy - state.noise_floor_energy;
        state.noise_floor_energy += div_round_i64(diff, NOISE_BETA_DIVISOR);
    }

    voiced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_i16(samples: &[f32]) -> Vec<i16> {
        samples.iter().map(|&s| s as i16).collect()
    }

    /// Bug found and fixed this pass: `is_voiced_fixed` divided
    /// `energy_acc / n` with no guard against `n == 0` -- an empty
    /// `samples` slice panicked with `attempt to divide by zero` in
    /// BOTH debug and `--release` builds (confirmed empirically before
    /// the fix: `is_voiced_fixed(&mut state, &[])` panicked with exactly
    /// that message). The crate's own one real caller
    /// (`EncoderFixed::encode`) never passes an empty slice, but this is
    /// a `pub fn` with no length precondition documented or enforced,
    /// so an empty buffer (e.g. an end-of-stream partial read, an
    /// off-by-one frame-size bug in a future caller) is a realistic,
    /// not contrived, way to reach this.
    #[test]
    fn is_voiced_fixed_does_not_panic_on_an_empty_slice() {
        let mut state = VoicingStateFixed::new();
        assert!(
            !is_voiced_fixed(&mut state, &[]),
            "an empty (no-signal) sample slice should be judged unvoiced, not panic"
        );
        // State must stay usable afterward -- a real caller could call
        // this again with real samples right after an empty one.
        let tone = to_i16(
            &super::super::floating_reference::voicing::tests::synthetic_tone(
                150.0, 8000.0, 80, 8000.0,
            ),
        );
        for _ in 0..10 {
            is_voiced_fixed(&mut state, &[0i16; 80]);
        }
        assert!(
            is_voiced_fixed(&mut state, &tone),
            "state should still work normally on real samples after an empty-slice call"
        );
    }

    /// `is_voiced_fixed` validated against the *same* real scenarios
    /// `is_voiced` itself is validated against
    /// (`floating_reference::voicing`'s own test module) -- the real
    /// check this candidate's own doc comment promises (a deliberately
    /// different EMA domain still needs to produce the same real
    /// decisions on real scenarios, not just be defensible on paper).
    #[test]
    // Tests [@ANCHOR: is_voiced_fixed]
    // Tests [@ANCHOR: div_round_i64]
    fn is_voiced_fixed_matches_is_voiced_on_every_real_scenario_this_module_is_validated_against() {
        use crate::codec2_3200::floating_reference::voicing::tests::{synthetic_tone, white_noise};

        // Scenario 1: a clean low-pitched tone is voiced once the noise
        // floor settles.
        let mut state = VoicingStateFixed::new();
        let silence = vec![0i16; 80];
        for _ in 0..10 {
            is_voiced_fixed(&mut state, &silence);
        }
        let tone = to_i16(&synthetic_tone(150.0, 8000.0, 80, 8000.0));
        assert!(
            is_voiced_fixed(&mut state, &tone),
            "fixed: a clean 150Hz tone at real speech amplitude should be judged voiced"
        );

        // Scenario 2: white noise is not voiced.
        let mut state = VoicingStateFixed::new();
        let mut seed = 42u32;
        for _ in 0..10 {
            is_voiced_fixed(&mut state, &silence);
        }
        let noise = to_i16(&white_noise(8000.0, 80, &mut seed));
        assert!(
            !is_voiced_fixed(&mut state, &noise),
            "fixed: broadband noise should not be judged voiced"
        );

        // Scenario 3: silence is not voiced.
        let mut state = VoicingStateFixed::new();
        for _ in 0..10 {
            assert!(!is_voiced_fixed(&mut state, &silence));
        }

        // Scenario 4: a quiet tone well below a loud settled noise floor
        // is not voiced.
        let mut state = VoicingStateFixed::new();
        let mut seed = 7u32;
        for _ in 0..15 {
            let noise = to_i16(&white_noise(4000.0, 80, &mut seed));
            is_voiced_fixed(&mut state, &noise);
        }
        let quiet_tone = to_i16(&synthetic_tone(150.0, 100.0, 80, 8000.0));
        assert!(
            !is_voiced_fixed(&mut state, &quiet_tone),
            "fixed: a tone too quiet relative to the noise floor should not be voiced"
        );
    }
}
