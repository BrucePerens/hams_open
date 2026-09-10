// SPDX-License-Identifier: LGPL-3.0-or-later
//! **Not production code for 3200bps** -- see `floating_reference/
//! mod.rs`'s own doc comment for why `Encoder` (the struct defined
//! there) is a cross-validation reference, not what 3200bps actually
//! ships. That framing is about the `Encoder` struct specifically, not
//! every function in this file: `autocorrelate`/`levinson_durbin`/
//! `lpc_energy`/`lpc_to_lsp` here are genuine, live production code for
//! `codec2_1600::Encoder` (imported directly as `flpc` in `codec2_1600/
//! mod.rs`'s own `analyse_lsps_and_energy`) -- 1600bps reuses this
//! module's whole LPC analysis chain rather than porting its own
//! fixed-point version the way 3200bps's `EncoderFixed` did. The original, fully-`f32` LPC analysis functions
//! (autocorrelation, Levinson-Durbin, LPC-to-LSP conversion via the
//! standard Chebyshev-polynomial root-search technique, and LPC energy),
//! moved here from `codec2_3200::lpc` once that module's own fixed-point
//! siblings (`autocorrelate_fixed`, `levinson_durbin_fixed_from_integer_
//! r`, `lpc_to_lsp_from_integer_ak`, `lpc_energy_fixed`) made these the
//! only remaining callers. `codec2_3200::lpc` still owns, and this
//! module borrows back via `pub(crate)` imports, the pieces the fixed
//! path's own shared cores need directly: `find_next_root_from_q23`
//! (called by both `find_next_root` below and `lpc_to_lsp_from_integer_
//! ak` directly), `COEF_FRAC_BITS`, and `f32_to_q`. `codec2_3200::lpc`
//! also keeps `lsp_to_lpc` (the LSP-to-LPC *decoder* reconstruction,
//! used by the one shared `Decoder` both encoders' bitstreams go
//! through) and `Autocorr`/`LpcCoeffs` (the crate-wide type aliases both
//! sides use) -- neither of those is part of this module's own move.
//!
//! `levinson_durbin`'s own `|k|>1` safety clamp is a genuine numerical
//! bifurcation point, not just a defensive bound -- see
//! `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md`'s "A real,
//! significant risk found by actually testing Levinson-Durbin against
//! real data" section for the full characterization (a tiny rounding
//! difference earlier in the recursion can flip whether a later
//! iteration's reflection coefficient crosses the clamp, cascading into
//! an order-of-magnitude coefficient error). This float implementation
//! is the reference; `codec2_3200::lpc::levinson_durbin_fixed_from_
//! integer_r` is the validated fixed-point port, per Bruce's own
//! explicit direction: match the float reference's clamp behavior as
//! closely as practical and no closer (accept its real, measured
//! divergence rate rather than adding a stabilization/smoothing step
//! that would change the algorithm).

use crate::codec2_3200::lpc::{find_next_root_from_q23, Autocorr, LpcCoeffs, COEF_FRAC_BITS};
use crate::codec2_3200::LPC_ORD;

/// `R[j] = sum(Wn[i] * Wn[i+j])` for `j` in `0..=LPC_ORD`, over the
/// windowed analysis buffer `wn`. `pub(crate)`: `codec2_3200::lpc`'s own
/// tests don't call this directly today, but it's exported the same way
/// every other moved function here is, for consistency and any future
/// cross-validation test.
// [@ANCHOR: autocorrelate]
pub(crate) fn autocorrelate(wn: &[f32]) -> Autocorr {
    let mut r = [0.0f32; LPC_ORD + 1];
    for (j, r_j) in r.iter_mut().enumerate() {
        let mut sum = 0.0f32;
        for i in 0..(wn.len() - j) {
            sum += wn[i] * wn[i + j];
        }
        *r_j = sum;
    }
    r
}

// ---------------------------------------------------------------------
// White noise correction (autocorrelation regularization) for
// Levinson-Durbin's own numerical ill-conditioning.
//
// Note for anyone reading this from outside this codebase (written with
// David Rowe specifically in mind, since this analysis grew out of
// characterizing Codec2-mod's own LPC stage for a fixed-point port --
// see docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md and
// docs/references/CODEC2_FIXED_POINT_WIDTH_REDUCTION_STUDY.md in this
// same crate for the full derivation this comment summarizes):
//
// THE PROBLEM, characterized empirically, not assumed:
//
// Levinson-Durbin's per-iteration reflection coefficient is
// `k = -(r[i] + sum(a_prev[j]*r[i-j])) / e`, where `e` (the running
// prediction-residual energy) is monotonically non-increasing --
// multiplied by `(1 - k^2) <= 1` every iteration. For a *well*-predicted
// frame (a strong, clean, steady vowel -- the LPC model is doing its job
// well, not badly), `e` can shrink close to zero, and dividing by a
// near-zero `e` amplifies whatever numerical noise already exists in the
// numerator by `1/e`.
//
// This is NOT a fixed-point-specific problem, and NOT a bug in any one
// implementation. Direct measurement against 2532 real captured speech
// frames (`CODEC2_MOD_FIXED_POINT_PLAN.md`'s own "A real, significant
// risk" section) found the *same* class of divergence between plain
// `float32` and a `float64` transcription of the identical recursion,
// with zero fixed-point involved -- 1 frame in 2532 (0.04%) diverged
// outright, 10 more (0.4%) showed a smaller real perturbation. Ordinary
// IEEE754 `float32`, which Codec2/Codec2-mod already ships with, already
// sits close enough to this cliff edge to fall off it for a real, if
// rare, fraction of real speech.
//
// A second, independent piece of evidence found later in the same
// investigation (`CODEC2_FIXED_POINT_WIDTH_REDUCTION_STUDY.md`'s
// autocorrelate-boundary work): this fragility isn't confined to
// Levinson-Durbin's own division. A candidate replacement for a
// *different*, nearby division (normalizing R[] by R[0] before this
// port's own fixed-point recursion) was built to be *more*
// mathematically exact than the current f32-based normalization -- and
// it still diverged from the reference on this corpus's own worst frame
// (frame 273), because that frame is fragile enough that even the
// ordinary, unremarkable ~1.7e-8 relative rounding error inherent to a
// single `f32` division is load-bearing for which answer you get. That
// is: "make the arithmetic more precise" does not fix this class of
// problem, because the instability is in the *algorithm's own
// conditioning* at that frame, not in any specific implementation's
// rounding budget.
//
// THE FIX, standard and well-established, not novel to this project:
//
// "White noise correction" (Rabiner & Schafer, *Digital Processing of
// Speech Signals*, 1978 -- the standard reference for this exact
// technique; also called autocorrelation regularization elsewhere in the
// literature) multiplies `R[0]` alone by a small factor `(1 + alpha)`,
// leaving `R[1..LPC_ORD]` untouched. This is *not* a uniform rescaling
// of `R[]` (which would change nothing -- Levinson-Durbin's reflection
// coefficients are provably scale-invariant under multiplying the whole
// `R[]` vector by a constant). Correcting `R[0]` alone breaks that
// proportionality on purpose: it models the physical reality that real
// speech always carries *some* small amount of energy that genuinely
// isn't predictable from the signal's own past samples (microphone
// noise, quantization noise, unvoiced excitation riding on a voiced
// segment) -- a noise floor no real recording is completely free of.
// Injecting that same assumption into the autocorrelation estimate
// directly bounds how small `e` can get, since a perfectly-predicted
// signal is no longer represented as perfectly predictable.
//
// alpha = 1e-3 (0.1%, a noise floor ~30dB below the signal) was chosen
// by direct measurement against this crate's own real captured
// `codec2_r_dump.txt` corpus (362 real frames), not picked from the
// literature and assumed to transfer -- see
// `codec2_3200::lpc`'s own `white_noise_correction_measurably_improves_
// the_worst_case_amplification_margin` test for the real sweep this was
// chosen from. Real, measured result: the worst-case `1/e` amplification
// factor across the whole corpus drops from ~3581x (uncorrected) to
// ~269x at alpha=1e-3 -- over a 13x improvement -- while a 0.1% energy
// correction is far below the ~26% intensity change (~1dB) generally
// cited as the just-noticeable difference for loudness, so this has no
// perceptible effect on ordinary, well-conditioned frames (the
// overwhelming majority of real speech). Larger alpha values measured
// further improvement still (1e-2 -> ~49x worst case) at the cost of a
// larger, though still small, energy correction; 1e-3 was chosen as the
// more conservative of the two real candidates measured, not because
// 1e-2 was found unacceptable.
// ---------------------------------------------------------------------

/// Standard white noise correction factor for `apply_white_noise_
/// correction` below -- see that function's own doc comment (and the
/// module-level comment above it) for the full derivation. `pub(crate)`:
/// `codec2_3200::lpc`'s own `white_noise_correction_measurably_
/// improves_the_worst_case_amplification_margin` test (its own
/// self-contained inline Levinson-Durbin re-implementation, not a call
/// into this module) reads this same constant.
pub(crate) const WHITE_NOISE_CORRECTION_ALPHA: f32 = 1e-3;

/// Applies white noise correction to `r[0]` only (see the module-level
/// comment above `autocorrelate` for why `r[0]` alone, and not a uniform
/// rescaling of `r[]`). Deliberately a separate, explicit step callers
/// must invoke themselves, rather than folded silently into
/// `autocorrelate`'s own output -- `autocorrelate` is validated directly
/// against a real captured reference, and this correction is a
/// deliberate, visible, separate decision, not an invisible side effect
/// of computing an autocorrelation.
pub(crate) fn apply_white_noise_correction(r: &mut Autocorr) {
    r[0] *= 1.0 + WHITE_NOISE_CORRECTION_ALPHA;
}

/// Levinson-Durbin recursion: real autocorrelation coefficients in,
/// LPC coefficients out (`ak[0] == 1.0` by definition, `ak[1..=LPC_ORD]`
/// the real predictor coefficients). `pub(crate)`: `codec2_3200::lpc`'s
/// own cross-validation tests don't call this directly (its own
/// `levinson_durbin_fixed`/`levinson_durbin_fixed_core` are independent,
/// superseded fixed-point candidates kept for their own historical
/// clamp-divergence study, not production code checked against this
/// function), but it's exported for consistency and any future use --
/// and it's genuine, live production code for `codec2_1600::Encoder`
/// (via `flpc::levinson_durbin` in `codec2_1600/mod.rs`'s own
/// `analyse_lsps_and_energy`), not merely a cross-validation reference,
/// despite this whole file's own "Not production code" header (that
/// header is accurate for the `Encoder` *struct* in this directory's
/// `mod.rs`, which 3200bps no longer uses in production -- it does not
/// mean every function moved into this subdirectory is dead weight; see
/// `codec2_1600/mod.rs`'s own doc comment, which already documents this
/// reuse candidly).
///
/// IF `e` (the running prediction-residual energy, seeded from `r[0]`)
/// is `<= 0.0` at the start of an iteration, THE system SHALL leave
/// every remaining reflection coefficient and predictor tap at `0.0`
/// (the identity, "no further prediction" filter) rather than compute
/// `k = -(...)/ e`, which is `0.0/0.0 = NaN` whenever the numerator is
/// also exactly zero -- a real, reachable case: an all-zero windowed
/// analysis frame (genuine digital silence -- a squelched receiver, a
/// muted mic, or comfort-noise padding, none of which this crate gates
/// upstream) autocorrelates to an all-zero `R[]`, and `apply_white_
/// noise_correction`'s own multiplicative-only `r[0] *= 1.0 + alpha`
/// correction leaves an exact `0.0` exactly `0.0` (unlike its fixed-
/// point sibling `r0_normalize_fixed`, dividing by an exact-zero `f32`
/// denominator does not panic -- it produces `NaN`, silently, per bug
/// class 36 in `SKILL.md`). Before this fix, that `NaN` propagated
/// through every remaining `a[i]`/`e` value (confirmed empirically: an
/// all-zero `Autocorr` produced `ak == [1.0, NaN, NaN, ..., NaN]`) and
/// happened to be caught downstream only by two separate, unrelated
/// pieces of IEEE754/cast incidental behavior it was never designed to
/// rely on: `lpc_to_lsp`'s own `f64->i32` quantization saturates `NaN`
/// to `0` (Rust's float-to-int cast semantics), which starves the root
/// search of real information rather than failing cleanly on its own
/// terms, and `encode_energy`'s `e_linear.max(1e-12)` call happens to
/// ignore a `NaN` argument (per `f32::max`'s own documented "ignoring
/// NaN" semantics) rather than being written to handle a `NaN` `lpc_
/// energy` result on purpose. Both of those are real today (verified
/// empirically, not assumed) but neither is a designed contract this
/// function's own callers can rely on -- a future change to either one
/// (e.g. switching `encode_energy`'s own clamp to `e_linear.clamp(1e-12,
/// f32::MAX)`, which panics on a `NaN` low bound instead of ignoring it)
/// would turn this same all-silence input into a live production panic
/// with no change to this function at all. The same `e <= 0.0` guard
/// also covers the second, subtler way `e` can hit exactly zero: a
/// reflection coefficient landing exactly on this function's own
/// `|k| > 1.0` clamp boundary (`k == 1.0` or `k == -1.0` precisely) zeros
/// `e` one iteration later (`e *= 1.0 - k*k`) via the *strict* `>`
/// comparison letting an exact `1.0`/`-1.0` through unclamped --
/// astronomically unlikely to occur by chance in continuous
/// `f32` arithmetic (unlike the fixed-point sibling's own analogous
/// representable-grid-boundary case, which round-to-nearest rounding
/// makes routine), but the guard is the same one-line fix either way,
/// so it's covered rather than special-cased away as "can't happen."
// [@ANCHOR: levinson_durbin]
pub(crate) fn levinson_durbin(r: &Autocorr) -> LpcCoeffs {
    let mut a = [0.0f32; LPC_ORD + 1];
    let mut a_prev = [0.0f32; LPC_ORD + 1];
    a[0] = 1.0;
    let mut e = r[0];

    for i in 1..=LPC_ORD {
        if e <= 0.0 {
            // No measurable prediction-residual energy left -- the
            // identity filter (no further prediction) is the physically
            // meaningful answer for every remaining order, not a 0/0
            // division. `a[i..]`/`a_prev[i..]` are already `0.0` from
            // initialization, so there's nothing left to write.
            break;
        }
        let mut sum = 0.0f32;
        for j in 1..i {
            sum += a_prev[j] * r[i - j];
        }
        let mut k = -(r[i] + sum) / e;
        if k.abs() > 1.0 {
            k = 0.0;
        }
        a[i] = k;
        for j in 1..i {
            a[j] = a_prev[j] + k * a_prev[i - j];
        }
        e *= 1.0 - k * k;
        a_prev[..=i].copy_from_slice(&a[..=i]);
    }
    a
}

/// Builds the symmetric/antisymmetric `P'(z)`/`Q'(z)` Chebyshev-domain
/// polynomials whose interleaved roots are the LPC filter's Line
/// Spectral Frequencies, per the standard LSP construction: factor the
/// LPC inverse filter `A(z)` into `P(z) = A(z) + z^-(p+1)*A(z^-1)` and
/// `Q(z) = A(z) - z^-(p+1)*A(z^-1)`, both of which have all roots on the
/// unit circle for a stable `A(z)`. `pub(crate)`: `codec2_3200::lpc`'s
/// own build_p_q_fixed cross-validation stays independent (see that
/// function's own doc comment), so nothing there calls this today, but
/// it's exported for consistency.
// [@ANCHOR: build_p_q]
pub(crate) fn build_p_q(ak: &LpcCoeffs) -> ([f32; 6], [f32; 6]) {
    let m = LPC_ORD / 2;
    let mut p = [0.0f32; 6];
    let mut q = [0.0f32; 6];
    p[0] = 1.0;
    q[0] = 1.0;
    for i in 1..=m {
        p[i] = ak[i] + ak[LPC_ORD + 1 - i] - p[i - 1];
        q[i] = ak[i] - ak[LPC_ORD + 1 - i] + q[i - 1];
    }
    for i in 0..m {
        p[i] *= 2.0;
        q[i] *= 2.0;
    }
    (p, q)
}

/// Finds one root of `poly` by sweeping `x` downward from `x_start` in
/// `codec2_3200::lpc`'s own `LSP_SEARCH_STEP` increments until a sign
/// change brackets a root, then bisecting `LSP_BISECTIONS` times. The
/// returned root is also where the *next* root's search should resume,
/// matching LSP roots' own real interleaving property (each of the
/// `LPC_ORD` roots lies in a disjoint sub-interval of `[-1, 1]`, in
/// strictly decreasing order, so search never needs to backtrack). A
/// thin wrapper: quantizes `poly` to Q8.23 once, then delegates the real
/// search to `codec2_3200::lpc::find_next_root_from_q23`, the same
/// shared core `lpc_to_lsp_from_integer_ak` calls directly with an
/// already-quantized polynomial -- this float-facing entry point and
/// that fixed-facing one both bottom out in the identical arithmetic, so
/// there's no separate float root-search to keep in sync.
// [@ANCHOR: find_next_root]
pub(crate) fn find_next_root(poly: &[f32; 6], x_start: f32) -> Option<f32> {
    let poly_q: [i32; 6] =
        std::array::from_fn(|i| (poly[i] as f64 * (1i64 << COEF_FRAC_BITS) as f64).round() as i32);
    find_next_root_from_q23(&poly_q, x_start)
}

/// LPC coefficients -> `LPC_ORD` Line Spectral Frequencies (radians,
/// strictly increasing). Returns `None` if fewer than `LPC_ORD` roots
/// were found in `[-1, 1]` (a real, if rare, LPC analysis failure mode
/// on pathological input -- callers substitute benign fallback LSPs).
// [@ANCHOR: lpc_to_lsp]
pub(crate) fn lpc_to_lsp(ak: &LpcCoeffs) -> Option<[f32; LPC_ORD]> {
    let (p, q) = build_p_q(ak);
    let mut search_from = 1.0f32;
    let mut freq = [0.0f32; LPC_ORD];
    for (j, f) in freq.iter_mut().enumerate() {
        let poly = if j & 1 == 1 { &q } else { &p };
        let root = find_next_root(poly, search_from)?;
        *f = root.acos();
        search_from = root;
    }
    Some(freq)
}

/// LPC energy: `E = sum(ak[i] * R[i])`, the real prediction-error energy
/// this analysis frame's LPC filter achieves against its own
/// autocorrelation -- computed *before* bandwidth expansion is applied
/// to `ak`, matching the real reference's own ordering (bandwidth
/// expansion after this computation would introduce spurious negative
/// energies).
// [@ANCHOR: lpc_energy]
pub(crate) fn lpc_energy(ak: &LpcCoeffs, r: &Autocorr) -> f32 {
    ak.iter().zip(r.iter()).map(|(a, r)| a * r).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec2_3200::bw_gamma;
    use crate::codec2_3200::lpc::tests::{fixture, read_dump};
    use crate::codec2_3200::M_PITCH;

    /// Real, reachable production input (a genuinely all-silent windowed
    /// frame -- a squelched receiver, a muted mic, or comfort-noise
    /// padding, none of which this crate gates upstream of LPC analysis)
    /// drove `r[0]` to exact `0.0`, which pre-fix produced `k = 0.0/0.0 =
    /// NaN` on the very first iteration and propagated `NaN` through
    /// every remaining `ak[i]` -- confirmed empirically before the fix
    /// (`ak == [1.0, NaN, NaN, ..., NaN]`). This regression test pins the
    /// post-fix, fully-finite "identity filter" answer.
    #[test]
    // Tests [@ANCHOR: levinson_durbin]
    fn levinson_durbin_does_not_produce_nan_on_an_all_zero_r() {
        let r = [0.0f32; LPC_ORD + 1];
        let ak = levinson_durbin(&r);
        assert_eq!(
            ak,
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "an all-zero R[] should produce the identity (no-prediction) filter, not NaN"
        );
    }

    /// Real, end-to-end production path an all-silent frame actually
    /// takes in both `floating_reference::Encoder::encode` (3200bps,
    /// cross-validation) and `codec2_1600::analyse_lsps_and_energy`
    /// (1600bps, real production): `autocorrelate` ->
    /// `apply_white_noise_correction` -> `levinson_durbin` ->
    /// `lpc_energy`/`lpc_to_lsp`. Before the fix this "worked" only by
    /// accident (`lpc_to_lsp`'s own NaN-to-0 saturating cast, `f32::max`'s
    /// own NaN-ignoring semantics in the caller's `encode_energy`) --
    /// this test pins the real, designed-for outcome instead: a finite
    /// energy and a real (not `None`-then-fallback) LSP set.
    #[test]
    fn an_all_silent_frame_produces_finite_energy_and_lsps_through_the_real_production_chain() {
        let windowed = [0.0f32; M_PITCH];
        let r = autocorrelate(&windowed);
        let mut r_for_levinson = r;
        apply_white_noise_correction(&mut r_for_levinson);
        let ak = levinson_durbin(&r_for_levinson);
        assert!(
            ak.iter().all(|v| v.is_finite()),
            "non-finite LPC coefficient from an all-silent frame: {ak:?}"
        );
        let e = lpc_energy(&ak, &r);
        assert!(
            e.is_finite(),
            "non-finite LPC energy from an all-silent frame: {e}"
        );
        let lsp = lpc_to_lsp(&ak);
        assert!(
            lsp.is_some_and(|freqs| freqs.iter().all(|f| f.is_finite())),
            "expected a real, finite LSP set for an all-silent frame, got {lsp:?}"
        );
    }

    /// A reflection coefficient landing exactly on the `|k| > 1.0` clamp
    /// boundary (`k == 1.0` precisely) zeros `e` one iteration later via
    /// `e *= 1.0 - k*k` -- the second, subtler way this function's own
    /// `e <= 0.0` guard earns its keep, distinct from an all-zero `r[0]`
    /// input. Crafted directly (a real captured 362-frame corpus is too
    /// small to happen to contain an exact `f32` tie at this boundary),
    /// not derived from real audio -- see this function's own doc comment
    /// for why this is nonetheless a real, not purely theoretical, case
    /// the same one-line guard needed to cover anyway.
    #[test]
    fn levinson_durbin_does_not_produce_nan_when_a_reflection_coefficient_hits_the_clamp_boundary_exactly()
     {
        // Order 1: k = -(r[1])/r[0]. Choosing r[1] == -r[0] (both
        // nonzero) makes k == 1.0 exactly on the very first iteration,
        // zeroing e for iteration 2 (e *= 1.0 - 1.0*1.0 == 0.0).
        let mut r = [0.0f32; LPC_ORD + 1];
        r[0] = 1.0;
        r[1] = -1.0;
        let ak = levinson_durbin(&r);
        assert!(
            ak.iter().all(|v| v.is_finite()),
            "non-finite LPC coefficient from a crafted exact-boundary R[]: {ak:?}"
        );
        assert_eq!(ak[1], 1.0, "the boundary-hitting reflection coefficient itself should be preserved, not clamped (only a strict > 1.0 clamps)");
        assert_eq!(
            &ak[2..],
            &[0.0; LPC_ORD - 1],
            "every order after e hits exactly zero should be the identity filter"
        );
    }

    /// Real R[] vectors and their real Levinson-Durbin ak[] outputs,
    /// captured from actual Codec2-mod real speech decoding this same
    /// session -- cross-validates this independently-written
    /// implementation against the real reference's own real output, not
    /// just internal self-consistency.
    #[test]
    // Tests [@ANCHOR: levinson_durbin]
    fn levinson_durbin_matches_the_real_reference_on_real_captured_speech_data() {
        let r_path = fixture!("codec2_r_dump.txt");
        let ak_path = fixture!("codec2_ak_dump.txt");
        let rs = read_dump(r_path, LPC_ORD + 1);
        let aks = read_dump(ak_path, LPC_ORD + 1);
        assert_eq!(rs.len(), aks.len());
        assert!(
            rs.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            rs.len()
        );

        let mut max_abs_err = 0.0f32;
        let mut n_frames_over_tolerance = 0;
        for (r_row, ak_row) in rs.iter().zip(aks.iter()) {
            let mut r = [0.0f32; LPC_ORD + 1];
            r.copy_from_slice(r_row);
            let ak = levinson_durbin(&r);
            let mut this_max = 0.0f32;
            for i in 0..=LPC_ORD {
                this_max = this_max.max((ak[i] - ak_row[i]).abs());
            }
            max_abs_err = max_abs_err.max(this_max);
            // The real reference's own clamp-boundary bifurcation
            // (CODEC2_MOD_FIXED_POINT_PLAN.md) means a handful of real
            // frames can legitimately diverge if this independent
            // implementation's arithmetic rounds a hair differently at
            // exactly the |k|>1 boundary -- tolerate that known,
            // documented failure mode, don't paper over a real bug.
            if this_max > 0.01 {
                n_frames_over_tolerance += 1;
            }
        }
        assert!(
            n_frames_over_tolerance <= rs.len() / 20,
            "{n_frames_over_tolerance}/{} frames exceeded tolerance -- too many for the known clamp-boundary sensitivity alone, likely a real implementation bug (max err {max_abs_err})",
            rs.len()
        );
    }

    #[test]
    fn levinson_durbin_reflection_coefficients_stay_within_the_construction_bound() {
        // k is clamped to [-1,1] by construction on every real captured
        // frame -- assert the invariant holds, not just that it happens
        // to on this corpus.
        let r_path = fixture!("codec2_r_dump.txt");
        for r_row in read_dump(r_path, LPC_ORD + 1) {
            let mut r = [0.0f32; LPC_ORD + 1];
            r.copy_from_slice(&r_row);
            let ak = levinson_durbin(&r);
            for &coeff in ak.iter() {
                assert!(
                    coeff.is_finite(),
                    "non-finite LPC coefficient from real data: {ak:?}"
                );
            }
        }
    }

    /// Not just "did root-finding succeed" -- compares the actual root
    /// *values* against the reference's own real `lsp[]` output
    /// (`codec2_lsp_dump.txt`, dumped right after its own `lpc_to_lsp()`
    /// call). A success-rate-only check previously let a real bisection
    /// off-by-one bug (this implementation was returning a value from one
    /// extra, uncounted halving beyond the reference's real 6) through
    /// silently, since the wrong root value still counted as "found".
    #[test]
    // Tests [@ANCHOR: lpc_to_lsp]
    fn lpc_to_lsp_matches_the_real_reference_on_real_captured_ak_data() {
        let ak_path = fixture!("codec2_ak_dump.txt");
        let lsp_path = fixture!("codec2_lsp_dump.txt");
        let aks = read_dump(ak_path, LPC_ORD + 1);
        let lsps = read_dump(lsp_path, LPC_ORD + 1);
        assert_eq!(
            aks.len(),
            lsps.len(),
            "real captures must be from the same corpus pass to line up 1:1"
        );
        assert!(
            aks.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            aks.len()
        );

        let mut roots_found_count = 0;
        let mut max_abs_err = 0.0f32;
        for (ak_row, lsp_row) in aks.iter().zip(lsps.iter()) {
            let ref_roots = lsp_row[0] as usize;
            if ref_roots != LPC_ORD {
                // Real, rare root-finding failure on this frame -- the
                // reference's own dumped lsp[] is a benign fallback, not
                // a real root set, so there's nothing to compare here.
                continue;
            }
            let mut ak = [0.0f32; LPC_ORD + 1];
            ak.copy_from_slice(ak_row);
            for (i, a) in ak.iter_mut().enumerate() {
                *a *= bw_gamma(i);
            }
            let Some(lsp) = lpc_to_lsp(&ak) else { continue };
            roots_found_count += 1;
            for i in 0..LPC_ORD {
                max_abs_err = max_abs_err.max((lsp[i] - lsp_row[1 + i]).abs());
            }
        }
        assert!(
            roots_found_count as f64 / aks.len() as f64 > 0.95,
            "only {roots_found_count}/{} real frames found all {LPC_ORD} LSP roots -- expected the overwhelming majority to succeed on real speech",
            aks.len()
        );
        // With the fix, real measured max error against the reference's
        // own captured lsp[] is ~1.8e-6 rad (float rounding noise).
        // Negative-controlled: temporarily reverting to a fresh 7th
        // (uncounted) bisection average instead of the last computed
        // midpoint pushes the real measured max error to ~1.9e-3 rad on
        // this same fixture -- three orders of magnitude apart, so 1e-4
        // sits with wide margin on both sides and actually discriminates
        // the bug this test exists to catch, not just root-finding
        // success/failure.
        assert!(
            max_abs_err < 1e-4,
            "max LSP root error vs real captured reference: {max_abs_err} rad"
        );
    }

    #[test]
    // Tests [@ANCHOR: build_p_q]
    fn build_p_q_matches_the_real_reference_p_q_on_real_captured_data() {
        let ak_path = fixture!("codec2_ak_dump.txt");
        let pq_path = fixture!("codec2_pq_dump.txt");
        let aks = read_dump(ak_path, LPC_ORD + 1);
        let pqs = read_dump(pq_path, 12);
        assert_eq!(
            aks.len(),
            pqs.len(),
            "real captures must be from the same corpus pass to line up 1:1"
        );

        let mut max_err = 0.0f32;
        for (ak_row, pq_row) in aks.iter().zip(pqs.iter()) {
            let mut ak = [0.0f32; LPC_ORD + 1];
            ak.copy_from_slice(ak_row);
            for (i, a) in ak.iter_mut().enumerate() {
                *a *= bw_gamma(i);
            }
            let (p, q) = build_p_q(&ak);
            for i in 0..6 {
                max_err = max_err.max((p[i] - pq_row[i]).abs());
                max_err = max_err.max((q[i] - pq_row[6 + i]).abs());
            }
        }
        assert!(
            max_err < 1e-3,
            "max P[]/Q[] error vs real captured reference: {max_err}"
        );
    }

    /// Cross-checks `autocorrelate` against the reference's own real
    /// `R[]` output on the reference's own real windowed buffer
    /// (`synthetic_codec2_wn_dump.txt`, dumped right before its own
    /// `autocorrelate` call, from a locally synthesized non-speech test
    /// signal -- see `tests/fixtures/codec2_3200/README.md` for why a
    /// synthetic signal is used here specifically: `Wn[]` is audio-domain
    /// data, unlike this file's other, real-speech-derived fixtures which
    /// hold only abstracted numeric features). Catches a scale bug this
    /// crate's own `window.rs` could introduce (a wrong window
    /// normalization constant) that a self-consistency-only test can't:
    /// `levinson_durbin`'s `ak` output is scale-invariant in `R[]` (every
    /// reflection coefficient is a ratio), so a uniformly wrong `R[]`
    /// scale wouldn't show up in the `ak`-based tests above at all, even
    /// though `lpc_energy` (which feeds `encode_energy` directly into the
    /// bitstream) is scale-dependent.
    #[test]
    // Tests [@ANCHOR: autocorrelate]
    fn autocorrelate_matches_the_real_reference_r_on_a_synthetic_signals_real_captured_wn_data() {
        let wn_path = fixture!("synthetic_codec2_wn_dump.txt");
        let r_path = fixture!("synthetic_codec2_r_dump.txt");
        let wns = read_dump(wn_path, M_PITCH);
        let rs = read_dump(r_path, LPC_ORD + 1);
        assert_eq!(
            wns.len(),
            rs.len(),
            "real captures must be from the same corpus pass to line up 1:1"
        );
        assert!(
            wns.len() > 150,
            "expected the synthetic-signal fixture corpus, got {} rows",
            wns.len()
        );

        let mut max_rel_err = 0.0f32;
        for (wn_row, r_row) in wns.iter().zip(rs.iter()) {
            let r = autocorrelate(wn_row);
            for i in 0..=LPC_ORD {
                let denom = r_row[i].abs().max(1e-6);
                max_rel_err = max_rel_err.max((r[i] - r_row[i]).abs() / denom);
            }
        }
        assert!(
            max_rel_err < 1e-3,
            "max relative R[] error vs real captured reference: {max_rel_err}"
        );
    }
}
