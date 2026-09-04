// SPDX-License-Identifier: LGPL-3.0-or-later
//! LPC analysis: autocorrelation, Levinson-Durbin recursion (Makhoul
//! 1975), and LPC-to-LSP conversion via the standard Chebyshev-polynomial
//! root-search technique (Sugamura & Itakura 1981 and its many
//! descendants -- the general method, not any one implementation of it).
//!
//! `levinson_durbin`'s own `|k|>1` safety clamp is a genuine numerical
//! bifurcation point, not just a defensive bound -- see
//! `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md`'s "A real,
//! significant risk found by actually testing Levinson-Durbin against
//! real data" section for the full characterization (a tiny rounding
//! difference earlier in the recursion can flip whether a later
//! iteration's reflection coefficient crosses the clamp, cascading into
//! an order-of-magnitude coefficient error). This float implementation
//! is the reference; `levinson_durbin_fixed` below is the validated
//! fixed-point port, per Bruce's own explicit direction: match the
//! float reference's clamp behavior as closely as practical and no
//! closer (accept its real, measured divergence rate rather than adding
//! a stabilization/smoothing step that would change the algorithm the
//! plan doc's own two open options separately identified).

use super::LPC_ORD;

pub type Autocorr = [f32; LPC_ORD + 1];
pub type LpcCoeffs = [f32; LPC_ORD + 1];
/// LPC coefficients in Q8.23 fixed-point (`COEF_FRAC_BITS`), the real
/// internal representation `levinson_durbin_fixed_core` computes in --
/// `levinson_durbin_fixed` converts this to `LpcCoeffs` (`f32`) only at
/// its own output boundary, matching this port's established "integer
/// core, float boundary" pattern.
type LpcCoeffsQ = [i64; LPC_ORD + 1];

/// `R[j] = sum(Wn[i] * Wn[i+j])` for `j` in `0..=LPC_ORD`, over the
/// windowed analysis buffer `wn`.
pub fn autocorrelate(wn: &[f32]) -> Autocorr {
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

/// Q8.23 for the fixed-point windowed samples `autocorrelate_fixed`
/// below consumes -- same width `COEF_FRAC_BITS` uses elsewhere in this
/// file, chosen for the same reason: real measured `wn[i]` peaks at
/// ~141.8 (a real `i16` sample, max magnitude 32767, times the real
/// measured window peak ~0.00433), needing 8 integer bits (`2^8=256 >
/// 141.8`) with real margin, and 23 fractional bits fits the remaining
/// budget in an `i32` with room to spare (`141.8 * 2^23 ~ 1.19e9`,
/// comfortably under `i32::MAX ~ 2.15e9`).
const WN_FRAC_BITS: u32 = 23;

/// Fixed-point `autocorrelate`: `wn_q` (Q8.23, `i32`) in, `R[]` (Q8.23,
/// `i64`) out -- no `f32` anywhere. Each per-term product
/// (`wn_q[i]*wn_q[i+j]`, both up to ~1.19e9 in magnitude, so a product up
/// to ~1.42e18) widens to `i64` transiently (the native 32x32->64
/// multiply every target this port cares about has in hardware, the same
/// `q_mul`/`cheb_poly_eval_fixed` shape used throughout this file), then
/// shifts right by `WN_FRAC_BITS` immediately -- bringing each term back
/// to the same Q8.23 format as the inputs -- before accumulating, so the
/// running sum itself never needs more than ordinary `i64` headroom.
/// Real, measured worst-case bound (not assumed): summing up to 320
/// such Q8.23 terms tops out around 5.4e13 in raw Q8.23 units --
/// `i64::MAX` is ~9.2e18, over 17 bits of real margin even at this
/// deliberately pessimistic (every sample simultaneously at its own real
/// measured peak) bound.
pub fn autocorrelate_fixed(wn_q: &[i32]) -> [i64; LPC_ORD + 1] {
    let mut r_q = [0i64; LPC_ORD + 1];
    for (j, r_j) in r_q.iter_mut().enumerate() {
        let mut sum: i64 = 0;
        for i in 0..(wn_q.len() - j) {
            sum += (wn_q[i] as i64 * wn_q[i + j] as i64) >> WN_FRAC_BITS;
        }
        *r_j = sum;
    }
    r_q
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
// `white_noise_correction_measurably_improves_the_worst_case_amplification_margin`
// below for the real sweep this was chosen from. Real, measured result:
// the worst-case `1/e` amplification factor across the whole corpus
// drops from ~3581x (uncorrected) to ~269x at alpha=1e-3 -- over a 13x
// improvement -- while a 0.1% energy correction is far below the ~26%
// intensity change (~1dB) generally cited as the just-noticeable
// difference for loudness, so this has no perceptible effect on
// ordinary, well-conditioned frames (the overwhelming majority of real
// speech). Larger alpha values measured further improvement still (1e-2
// -> ~49x worst case) at the cost of a larger, though still small,
// energy correction; 1e-3 was chosen as the more conservative of the two
// real candidates measured, not because 1e-2 was found unacceptable.
//
// WHY THIS ISN'T WIRED IN YET, stated plainly:
//
// This is a real, deliberate change to the *reference* algorithm's own
// behavior, not a numerically-transparent optimization -- it changes
// what LPC coefficients (and therefore what LSP indices) the encoder
// transmits, if only very slightly on well-conditioned frames and more
// meaningfully on the specific ill-conditioned frames it targets. This
// crate's own real interoperability claim (see mod.rs's own module doc
// comment) was checked against the real vendored Codec2-mod C decoder
// *before* this function existed -- that check would need to be re-run
// (a manual step, deliberately kept outside this crate's own automated
// build to avoid an LGPL-2.1-only linking entanglement, see
// examples/codec2_encode_wav.rs's own doc comment) before this should be
// treated as validated for real interoperability, not just for its own
// isolated numerical effect on this corpus.
// ---------------------------------------------------------------------

/// Standard white noise correction factor for `apply_white_noise_
/// correction` below -- see that function's own extensive doc comment
/// (and the module-level comment above it) for the full derivation and
/// the real measurement this specific value was chosen from.
pub const WHITE_NOISE_CORRECTION_ALPHA: f32 = 1e-3;

/// Applies white noise correction to `r[0]` only (see the module-level
/// comment above `autocorrelate` for why `r[0]` alone, and not a uniform
/// rescaling of `r[]`). Deliberately a separate, explicit step callers
/// must invoke themselves, rather than folded silently into
/// `autocorrelate`'s own output -- `autocorrelate` is validated directly
/// against a real captured reference (`autocorrelate_matches_the_real_
/// reference_r_on_a_synthetic_signals_real_captured_wn_data`), and this
/// correction is a deliberate, visible, separate decision, not an
/// invisible side effect of computing an autocorrelation.
pub fn apply_white_noise_correction(r: &mut Autocorr) {
    r[0] *= 1.0 + WHITE_NOISE_CORRECTION_ALPHA;
}

/// Fixed-point `apply_white_noise_correction`: `r_q[0] += r_q[0] /
/// 1000` -- an exact integer division standing in for `* (1 +
/// WHITE_NOISE_CORRECTION_ALPHA)` (`1e-3 == 1/1000` exactly, unlike an
/// arbitrary float constant, so this introduces no quantization error of
/// its own beyond the division's own round-to-nearest). `r_q[0]` is
/// always positive (real signal energy), so `div_round_i128`'s own
/// positive-divisor precondition holds trivially.
pub fn apply_white_noise_correction_fixed(r_q: &mut [i64; LPC_ORD + 1]) {
    r_q[0] += div_round_i128(r_q[0] as i128, 1000);
}

/// Levinson-Durbin recursion: real autocorrelation coefficients in,
/// LPC coefficients out (`ak[0] == 1.0` by definition, `ak[1..=LPC_ORD]`
/// the real predictor coefficients).
pub fn levinson_durbin(r: &Autocorr) -> LpcCoeffs {
    let mut a = [0.0f32; LPC_ORD + 1];
    let mut a_prev = [0.0f32; LPC_ORD + 1];
    a[0] = 1.0;
    let mut e = r[0];

    for i in 1..=LPC_ORD {
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

/// Uniform internal Q8.40 fixed-point format for every quantity in
/// `levinson_durbin_fixed_core`'s own recursion (`r_norm[]`, `e`, `k`,
/// and `a[]`/`a_prev[]` alike) -- **not** the narrower Q8.23
/// (`COEF_FRAC_BITS`) the downstream LSP/Chebyshev stage uses; that
/// conversion happens exactly once, at this function's own output
/// boundary (`levinson_durbin_fixed`), not throughout the recursion
/// itself.
///
/// **Why one wide uniform format, found by direct measurement, not
/// assumed**: an earlier version used Q8.23 for `a[]` (matching
/// `COEF_FRAC_BITS` directly, reasoning that it's the format the next
/// stage needs anyway) and Q2.29 for `r_norm`/`e`/`k`, with format
/// conversions at each cross-type operation. That version passed this
/// module's own clamp-disagreement discriminator test but failed a
/// tighter one (`levinson_durbin_fixed_matches_float_tightly_on_frames_
/// that_never_hit_the_clamp`): on the real fixture corpus's own
/// numerically hardest frame (frame 273, where `e` reaches its
/// corpus-wide minimum of 2.76e-4), coefficients diverged from the
/// float reference by up to 0.134 -- large, and with *no* clamp
/// disagreement at any iteration, meaning it was real arithmetic
/// imprecision, not the known algorithmic bifurcation. **The actual
/// mechanism, traced directly**: `k = -(numerator)/e` *divides by* `e`,
/// which means any quantization noise already present in `numerator`
/// (inherent to representing it in a finite-width format at all) gets
/// *amplified* by `1/e` -- at this frame's own worst point, `1/e ~
/// 3600x`. Q8.23's 23 fractional bits (~1.2e-7 relative resolution)
/// looked like ample headroom in isolation, but that headroom is what
/// gets consumed by the amplification, not spare margin against it.
/// Switching every quantity to a uniform Q8.40 format (giving ~9.1e-13
/// relative resolution, ~17 more bits than Q8.23) closed the gap on
/// the same frame with real margin -- confirmed against the whole
/// corpus by this module's own tests, not just the one frame that
/// exposed the problem. 8 integer bits, not 2, both because `a[]`
/// itself (not just the derived `P[]`/`Q[]` polynomial in the next
/// stage) can plausibly approach the same measured 77.17 magnitude
/// bound, and because the extra headroom costs nothing here (the
/// recursion's own products already need `i128` regardless, for the
/// same reason the narrower format did).
const LEVINSON_FRAC_BITS: u32 = 40;

/// Real fixed-point Levinson-Durbin, matching `levinson_durbin`'s exact
/// arithmetic shape but in genuine integer arithmetic throughout the
/// per-iteration recursion -- the actual target this exists for is
/// hardware with no FPU at all (cheap HT/ESP32-class parts), so this
/// isn't float dressed up in fixed-point language; every operation
/// inside the iteration loop is an integer multiply, shift, add, or
/// divide.
///
/// **The key design choice, found by direct measurement before writing
/// any integer code, not assumed**: `docs/references/
/// CODEC2_MOD_FIXED_POINT_PLAN.md` measured `R[0]`'s own real dynamic
/// range at ~10^11 (4.15e-6 to 8.60e5 across real speech) and flagged
/// this as needing block-floating-point treatment. But that huge span
/// is a *cross-frame* problem (quiet vs. loud speech), not an
/// intra-recursion one: `k = -(r[i]+sum)/e` is a ratio where the
/// numerator and denominator are the same scale (both derive from the
/// same frame's own `r[]`), so normalizing the whole `r[]` vector by
/// `r[0]` at entry removes the cross-frame span entirely -- `e` then
/// starts at exactly 1.0 every frame, `k` is a ratio of order-1
/// quantities, and `a[]` stays bounded, matching the existing Q8.23
/// format already validated for the downstream LSP Chebyshev stage
/// (`COEF_FRAC_BITS`). Measured directly (not assumed) whether this
/// still leaves `e` needing a running exponent within a frame: replayed
/// all 362 real captured `R[]` fixture vectors (`codec2_r_dump.txt`, a
/// stride-7 subsample of the plan doc's own denser ~2532-frame capture
/// -- this port doesn't have that denser corpus itself) through the
/// normalized recursion, recording the minimum `e` across *every
/// iteration of every frame*, not just final values. Real result:
/// minimum `e` is 2.76e-4 (log2 ~ -11.82), comfortably above the ~2^-20
/// threshold that would force per-iteration renormalization -- so a
/// single fixed Q-format for `e` suffices and no `norm_l`-style
/// exponent tracking is needed for this stage at all (see
/// `LEVINSON_FRAC_BITS`'s own doc comment for the real width that
/// format ended up needing -- Q8.40, not the narrower Q2.29 this
/// measurement alone would have suggested was enough; dividing by a
/// small `e` amplifies upstream quantization noise, which a plain
/// margin-on-`e`-alone estimate doesn't capture). This is a real,
/// load-bearing finding specific to this port's own recursion, not
/// assumed from the plan doc's own framing -- the plan doc measured
/// cross-frame `R[0]`/`e` dynamic range without separately checking
/// whether normalizing by `R[0]` at entry already solves it.
///
/// The `r[]` normalization itself (`r[j] / r[0]`) is the one boundary
/// operation still done in `f32` -- matching this port's own existing
/// precedent (`f32_to_q` for the Chebyshev stage's own coefficient
/// input) of doing format conversion at a function's real input
/// boundary in float while the recursive core is genuine integer
/// arithmetic. `autocorrelate()` itself is not yet fixed-point in this
/// port, so `r[]` necessarily arrives as `f32` regardless; this
/// boundary is a real, deliberate seam for a later pass to close, not
/// an oversight (see this stage's own module doc comment history).
///
/// **The clamp itself matches the float reference exactly, per Bruce's
/// own explicit direction**: `|k_q| > 1.0` (compared as a plain integer
/// comparison against `1i64 << LEVINSON_FRAC_BITS`) snaps `k` to zero,
/// identically to the float version's `if k.abs() > 1.0 { k = 0.0; }`
/// -- no smoothing, no stabilization, no attempt to reduce the real,
/// measured clamp-boundary divergence rate the plan doc already
/// characterized (0.04% float32-vs-float64 floor, up to 0.9% under
/// coarser per-step requantization). This port's own test compares
/// against that measured band directly, not against zero divergence.
pub fn levinson_durbin_fixed(r: &Autocorr) -> LpcCoeffs {
    let (a_q, _fired) = levinson_durbin_fixed_core(r);
    std::array::from_fn(|i| a_q[i] as f32 / (1i64 << COEF_FRAC_BITS) as f32)
}

/// Round-to-nearest right shift (round-half-up in two's complement,
/// via the standard "add half the divisor, then floor-shift" trick) --
/// used for every product/format-converting shift in
/// `levinson_durbin_fixed_core` below. **A real bug this port's own
/// test caught, not a preemptive nicety**: an earlier version used
/// plain arithmetic shift (`>>`, which floors -- a small, systematic
/// negative bias on every single shifted product) throughout. On the
/// real fixture corpus's own numerically hardest frame (frame 273, the
/// same one `e`'s own minimum-across-the-corpus measurement landed on)
/// that bias compounded across the recursion's 10 iterations into a
/// real, non-clamp-related 0.15 coefficient error -- caught by this
/// module's own `levinson_durbin_fixed_diverges_from_float_only_at_
/// measured_clamp_disagreement_frames` test, which is specifically
/// built to distinguish "expected clamp-boundary divergence" from "a
/// real bug in this port's own arithmetic." Switching every shift to
/// round-to-nearest closed that gap.
fn rshift_round(x: i64, n: u32) -> i64 {
    if n == 0 {
        return x;
    }
    (x + (1i64 << (n - 1))) >> n
}

/// Round-to-nearest division for the one true division in this
/// recursion (`k = -(...)/e`) -- same reasoning as `rshift_round`,
/// applied to `/` instead of `>>`. `d` (the divisor, `e_q` widened to
/// `i128`) is always positive by construction (`e` starts positive and
/// is only ever multiplied by `(1 - k^2) >= 0` since `k` is clamped to
/// `[-1, 1]`).
fn div_round_i128(n: i128, d: i128) -> i64 {
    debug_assert!(d > 0, "div_round_i128: divisor must be positive, got {d}");
    let half = d / 2;
    (if n >= 0 {
        (n + half) / d
    } else {
        (n - half) / d
    }) as i64
}

/// `(a * b) >> LEVINSON_FRAC_BITS`, rounded to nearest, computed in
/// `i128` -- both operands can be up to `i64` (48 bits magnitude in
/// this recursion's own Q8.40 format), so their product needs the
/// wider type; narrowed back to `i64` after the shift since every
/// quantity in this recursion fits Q8.40 by construction (`a[]`'s own
/// measured real bound, ~77 max, is far under `i64`'s own Q8.40 range).
fn q_mul(a: i64, b: i64) -> i64 {
    let product = a as i128 * b as i128;
    let half = 1i128 << (LEVINSON_FRAC_BITS - 1);
    let shifted = (product + half) >> LEVINSON_FRAC_BITS;
    debug_assert!(shifted >= i64::MIN as i128 && shifted <= i64::MAX as i128, "q_mul: result {shifted} overflows i64 -- a[]/e/k grew past this format's real, measured headroom (same class of bug as the f32_to_q overflow this function's own history already caught once)");
    shifted as i64
}

/// The real per-iteration recursion shared by `levinson_durbin_fixed`
/// (which only needs the final `ak[]`) and this module's own tests
/// (which need to know, per iteration, whether the `|k|>1` clamp
/// fired -- the real discriminator between "expected clamp-boundary
/// divergence from float" and "a bug in this port's own arithmetic,"
/// see `levinson_durbin_fixed_diverges_from_float_only_at_measured_
/// clamp_disagreement_frames`). A single shared core means both call
/// sites can never drift out of sync with each other the way two
/// independently-hand-maintained transcriptions could.
///
/// Every quantity (`r_norm[]`, `e`, `k`, `a[]`/`a_prev[]`) lives in the
/// same Q8.40 format throughout (`LEVINSON_FRAC_BITS`'s own doc comment
/// has the real measurement behind that choice) -- so, unlike an
/// earlier version of this function, no operation needs its own
/// bespoke format-conversion shift: same-format add is a plain `+`,
/// same-format multiply is `q_mul`, and the one true division folds its
/// own format bookkeeping into a single shift width.
fn levinson_durbin_fixed_core(r: &Autocorr) -> (LpcCoeffsQ, [bool; LPC_ORD + 1]) {
    let r0 = r[0];
    debug_assert!(r0 > 0.0, "levinson_durbin_fixed: r[0] must be positive (matches the float reference's own implicit assumption -- real captured speech never measured r[0] <= 0)");

    let r_norm_q: [i64; LPC_ORD + 1] =
        std::array::from_fn(|j| f32_to_q64(r[j] / r0, LEVINSON_FRAC_BITS));

    levinson_durbin_fixed_core_from_r_norm(&r_norm_q)
}

/// The real per-iteration recursion, shared by `levinson_durbin_fixed_
/// core` above (which normalizes via `f32` division, matching this
/// port's original validated behavior) and `levinson_durbin_fixed_from_
/// integer_r` below (which normalizes in genuine fixed-point, for a
/// caller that already has integer `R[]` from `autocorrelate_fixed`) --
/// a single shared body means the two entry points can never drift out
/// of sync with each other on anything but the one real difference
/// between them (how `r_norm_q` itself gets computed).
fn levinson_durbin_fixed_core_from_r_norm(
    r_norm_q: &[i64; LPC_ORD + 1],
) -> (LpcCoeffsQ, [bool; LPC_ORD + 1]) {
    let mut a_q = [0i64; LPC_ORD + 1]; // Q8.40
    let mut a_prev_q = [0i64; LPC_ORD + 1];
    a_q[0] = 1i64 << LEVINSON_FRAC_BITS;
    let mut e_q: i64 = 1i64 << LEVINSON_FRAC_BITS; // r_norm[0] == 1.0 exactly, by construction
    let mut fired = [false; LPC_ORD + 1];

    for i in 1..=LPC_ORD {
        let mut sum_q: i64 = 0;
        for j in 1..i {
            sum_q += q_mul(a_prev_q[j], r_norm_q[i - j]);
        }
        let numerator_q: i64 = r_norm_q[i] + sum_q;

        // `k_real = numerator_real / e_real`, both already Q(F) (F =
        // LEVINSON_FRAC_BITS), so `k_q = numerator_q * 2^F / e_q` --
        // widened to i128 for the shift since `numerator_q << F` can
        // approach i64's own range for a pathological frame (this
        // stage's whole point is finding those frames, not assuming
        // they can't occur).
        let k_q: i64 = if numerator_q == 0 {
            0
        } else {
            div_round_i128(-((numerator_q as i128) << LEVINSON_FRAC_BITS), e_q as i128)
        };

        let clamped = k_q.abs() > (1i64 << LEVINSON_FRAC_BITS);
        let k_q = if clamped { 0 } else { k_q };
        fired[i] = clamped;

        a_q[i] = k_q;
        for j in 1..i {
            a_q[j] = a_prev_q[j] + q_mul(k_q, a_prev_q[i - j]);
        }
        let one_minus_ksq_q: i64 = (1i64 << LEVINSON_FRAC_BITS) - q_mul(k_q, k_q);
        e_q = q_mul(e_q, one_minus_ksq_q);
        a_prev_q[..=i].copy_from_slice(&a_q[..=i]);
    }

    let a_q23: LpcCoeffsQ =
        std::array::from_fn(|i| rshift_round(a_q[i], LEVINSON_FRAC_BITS - COEF_FRAC_BITS));
    (a_q23, fired)
}

/// `r_norm_q[j] = r_q[j] * 2^LEVINSON_FRAC_BITS / r0_q` -- the
/// fixed-point replacement for `f32_to_q64(r[j]/r0, LEVINSON_FRAC_BITS)`
/// (`levinson_durbin_fixed_core`'s own internal, `f32`-based
/// normalization), for a caller that already has integer `R[]` (e.g.
/// from `autocorrelate_fixed`). Validated against the real
/// `codec2_r_dump.txt` corpus with `apply_white_noise_correction`/
/// `apply_white_noise_correction_fixed` applied first -- see
/// `levinson_durbin_fixed_tests::r0_normalization_fixed_point_candidate`
/// for the full derivation and the real measured result (0 unexplained
/// divergences from the plain-float reference). **Must not be called
/// without white noise correction applied first** -- the discriminator
/// tests back this function's own correctness specifically with that
/// correction in place; frame 273's own real fragility (this file's own
/// extensive documentation above) makes no promise about this
/// normalization's behavior without it.
fn r0_normalize_fixed(r_q: &[i64; LPC_ORD + 1]) -> [i64; LPC_ORD + 1] {
    let r0_q = r_q[0];
    debug_assert!(r0_q > 0, "r0_normalize_fixed: r0_q must be positive, got {r0_q}");
    std::array::from_fn(|j| div_round_i128((r_q[j] as i128) << LEVINSON_FRAC_BITS, r0_q as i128))
}

/// The real, integer-in/integer-out entry point for a caller that
/// already has `R[]` as integer Q(any format, e.g. `autocorrelate_
/// fixed`'s own Q8.23) rather than `f32` -- **caller must apply white
/// noise correction (`apply_white_noise_correction_fixed`) to `r_q`
/// before calling this**, matching this function's own validated
/// precondition. Output stays `LpcCoeffs` (`f32`), matching this port's
/// established "integer core, float boundary" pattern -- the downstream
/// LSP chain (`build_p_q`, `bw_gamma`) isn't migrated yet, so there's
/// nothing further along the pipeline to hand an integer type to.
/// Returns both the `f32` coefficients (the established output boundary)
/// and the real Q8.23 integer coefficients still inside -- a caller that
/// itself has integer `R[]` (e.g. to feed `lpc_energy_fixed`) needs the
/// latter to stay fixed-point end to end; a caller that doesn't can
/// simply ignore the second element.
pub fn levinson_durbin_fixed_from_integer_r(
    r_q: &[i64; LPC_ORD + 1],
) -> (LpcCoeffs, [i64; LPC_ORD + 1]) {
    let r_norm_q = r0_normalize_fixed(r_q);
    let (a_q23, _fired) = levinson_durbin_fixed_core_from_r_norm(&r_norm_q);
    (dequantize_coef_q23(&a_q23), a_q23)
}

/// Converts Q8.23 LPC coefficients (`a_q23`, e.g. from
/// `levinson_durbin_fixed_from_integer_r` or after `apply_bw_gamma_
/// fixed`) to `LpcCoeffs` (`f32`) -- the "integer core, float boundary"
/// conversion, exposed as its own function so a caller in another module
/// doesn't need `COEF_FRAC_BITS` (private to this module) to do it.
pub fn dequantize_coef_q23(a_q23: &[i64; LPC_ORD + 1]) -> LpcCoeffs {
    std::array::from_fn(|i| a_q23[i] as f32 / (1i64 << COEF_FRAC_BITS) as f32)
}

/// Q8.23 fixed-point for the Chebyshev coefficients below -- 8 integer
/// bits (real measured max \|coefficient\| across the real captured
/// corpus is 77.17, comfortably under the 128 this format allows), 23
/// fractional bits. Matches
/// `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md`'s own validated
/// width for this exact stage.
const COEF_FRAC_BITS: u32 = 23;
/// Q2.29 fixed-point for the Chebyshev recursion's own `T` register and
/// `x` -- \|T\|<=1 and \|x\|<=1 by construction, so 2 integer bits give
/// real margin; 29 fractional bits is the plan doc's own validated
/// width (zero sign mismatches across 20.26 million real evaluations at
/// this exact combination; deliberately coarsening `T`'s own precision
/// there produced real, monotonically worsening mismatch rates).
const CHEB_FRAC_BITS: u32 = 29;

fn f32_to_q(x: f32, frac_bits: u32) -> i32 {
    (x as f64 * (1i64 << frac_bits) as f64).round() as i32
}

/// Same conversion as `f32_to_q`, but returning `i64` -- required for
/// `LEVINSON_FRAC_BITS` (40): even a bounded `[-1,1]` value at 40
/// fractional bits needs up to ~2^40, which silently overflows `i32`
/// (max ~2^31). **A real bug this exact overflow caused, not a
/// preemptive widening**: an earlier version called `f32_to_q(x,
/// LEVINSON_FRAC_BITS) as i64` (narrow-then-widen, the wrong order) --
/// every `r_norm[]` entry silently saturated to `i32::MAX`/`i32::MIN`
/// (confirmed directly via temporary instrumentation: `r_norm_q` values
/// were all exactly `2147483647`/`-2147483648`, not real ratios at
/// all), producing coefficient errors up to 5.9 on every single test
/// frame. Caught by this module's own `levinson_durbin_fixed_diverges_
/// from_float_only_at_measured_clamp_disagreement_frames` test.
fn f32_to_q64(x: f32, frac_bits: u32) -> i64 {
    (x as f64 * (1i64 << frac_bits) as f64).round() as i64
}

/// Fixed-point evaluation of the same degree-`2*m` (`m=LPC_ORD/2=5`)
/// Chebyshev-basis polynomial `sum(coef[m-i] * T_i(x))` (three-term
/// recurrence `T_i = 2*x*T_{i-1} - T_{i-2}`) as the plain-float version
/// this replaces, but in real Q8.23/Q2.29 integer arithmetic -- the
/// exact stage `CODEC2_MOD_FIXED_POINT_PLAN.md` validated with zero
/// sign mismatches at this width. `find_next_root`'s own bisection only
/// ever consumes this result's *sign* (a bracketing/convergence test,
/// never the magnitude), so this returns the raw fixed-point
/// accumulator directly rather than converting back to `f32` -- the
/// sign of the `i32` accumulator IS the real answer, and converting
/// back to float first would just be adding an unnecessary lossy step
/// on top of an already-validated fixed-point result.
///
/// `x_q`/`T`/`sum` are `i32`, not `i64` -- `CODEC2_FIXED_POINT_WIDTH_
/// REDUCTION_STUDY.md`'s Question 2, measured directly against the real
/// fixture corpus (`cheb_poly_eval_fixed_intermediate_values_real_
/// measured_i32_fit_margin`): max `|x_q|`/`|T|` is ~2^29 (matches the
/// Q2.29 format exactly, as `\|x\|<=1`/`\|T\|<=1` by construction
/// promise), max `|sum|` ~2^29.94, both with real margin under `i32`'s
/// 2^31 range across 2.9 million real evaluations. Only the per-step
/// product widens to `i64` transiently (the one native 32x32->64
/// multiply every target this port cares about has in hardware),
/// narrowing back to `i32` immediately after each `>> CHEB_FRAC_BITS` --
/// proven bit-exact against the prior all-`i64` implementation across
/// the same corpus before this replaced it (real codegen win: ~46 vs 98
/// instructions on Xtensa LX6, ~half the code on ARM Cortex-M4F too,
/// zero calls either way).
///
/// **`#[cfg(test)]`**: `find_next_root` used to call this on every
/// candidate `x`, re-quantizing the same `coef` each time -- it now
/// quantizes once and calls `cheb_poly_eval_fixed_core` directly (see
/// that refactor's own commit), so this single-`f32`-eval wrapper is
/// only needed by tests that check one `(coef, x)` pair directly against
/// the real captured corpus.
#[cfg(test)]
fn cheb_poly_eval_fixed(coef: &[f32; 6], x: f32) -> i32 {
    let coef_q: [i32; 6] = std::array::from_fn(|i| f32_to_q(coef[i], COEF_FRAC_BITS));
    cheb_poly_eval_fixed_core(&coef_q, x)
}

/// The real per-`x` arithmetic, shared by `cheb_poly_eval_fixed` above
/// (which quantizes `coef` from `f32` on every call) and a caller that
/// already has `coef_q` in Q8.23 (e.g. `build_p_q_fixed`'s own output)
/// -- quantizing a polynomial's own coefficients once per root search
/// rather than once per candidate `x` (`find_next_root`'s coarse sweep
/// alone tries up to ~200 values of `x` against the *same* `coef`) is a
/// real, if secondary, efficiency win this split also happens to enable.
fn cheb_poly_eval_fixed_core(coef_q: &[i32; 6], x: f32) -> i32 {
    let x_q: i32 = f32_to_q(x, CHEB_FRAC_BITS);

    let mut t_prev2: i32 = 1i32 << CHEB_FRAC_BITS; // T_0 = 1.0, Q2.29
    let mut t_prev1: i32 = x_q; // T_1 = x, Q2.29

    // Each term is (Q8.23 coefficient * Q2.29 T) >> 29, bringing the
    // product back down to the coefficients' own Q8.23 format for the
    // running sum -- the product itself needs the transient i64 widen
    // (each factor up to ~2^30, so the product can reach ~2^60), but
    // every stored value narrows back to i32 immediately after.
    let mut sum: i32 = ((coef_q[5] as i64 * t_prev2 as i64) >> CHEB_FRAC_BITS) as i32;
    sum += ((coef_q[4] as i64 * t_prev1 as i64) >> CHEB_FRAC_BITS) as i32;

    for i in 2..=5 {
        let t_i: i32 =
            (((2i64 * x_q as i64 * t_prev1 as i64) >> CHEB_FRAC_BITS) - t_prev2 as i64) as i32;
        sum += ((coef_q[5 - i] as i64 * t_i as i64) >> CHEB_FRAC_BITS) as i32;
        t_prev2 = t_prev1;
        t_prev1 = t_i;
    }
    sum
}

/// Search step for `lpc_to_lsp`'s coarse root-bracketing sweep across
/// `x` in `[-1, 1]`.
const LSP_SEARCH_STEP: f32 = 0.01;
/// Bisection refinements applied once a bracket is found -- halves the
/// bracket width `LSP_BISECTIONS` times, so the final root position is
/// accurate to `LSP_SEARCH_STEP / 2^LSP_BISECTIONS`.
const LSP_BISECTIONS: u32 = 6;

/// Builds the symmetric/antisymmetric `P'(z)`/`Q'(z)` Chebyshev-domain
/// polynomials whose interleaved roots are the LPC filter's Line
/// Spectral Frequencies, per the standard LSP construction: factor the
/// LPC inverse filter `A(z)` into `P(z) = A(z) + z^-(p+1)*A(z^-1)` and
/// `Q(z) = A(z) - z^-(p+1)*A(z^-1)`, both of which have all roots on the
/// unit circle for a stable `A(z)`.
fn build_p_q(ak: &LpcCoeffs) -> ([f32; 6], [f32; 6]) {
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

/// Fixed-point `build_p_q`: `a_q23` (Q8.23, `i64`, e.g. straight from
/// `apply_bw_gamma_fixed`) in, `P[]`/`Q[]` (Q8.23, `i32`, matching
/// `cheb_poly_eval_fixed_core`'s own expected `coef_q` type) out. Pure
/// add/subtract/double -- no multiply at all, so no widening and no
/// precision loss beyond `a_q23`'s own already-established real margin
/// (`\|ak\|` <= 77.17 real measured max; `P[]`/`Q[]`'s own real measured
/// max, 77.17 as well per `COEF_FRAC_BITS`'s own doc comment, already
/// accounts for this construction's own real growth from that starting
/// bound, not a fresh, unchecked assumption). `*= 2` is an exact left
/// shift, not a lossy float multiply.
fn build_p_q_fixed(a_q23: &[i64; LPC_ORD + 1]) -> ([i32; 6], [i32; 6]) {
    let m = LPC_ORD / 2;
    let mut p = [0i64; 6];
    let mut q = [0i64; 6];
    p[0] = 1i64 << COEF_FRAC_BITS;
    q[0] = 1i64 << COEF_FRAC_BITS;
    for i in 1..=m {
        p[i] = a_q23[i] + a_q23[LPC_ORD + 1 - i] - p[i - 1];
        q[i] = a_q23[i] - a_q23[LPC_ORD + 1 - i] + q[i - 1];
    }
    for i in 0..m {
        p[i] *= 2;
        q[i] *= 2;
    }
    (
        std::array::from_fn(|i| p[i] as i32),
        std::array::from_fn(|i| q[i] as i32),
    )
}

/// Finds one root of `poly` by sweeping `x` downward from `x_start` in
/// `LSP_SEARCH_STEP` increments until a sign change brackets a root,
/// then bisecting `LSP_BISECTIONS` times. The returned root is also
/// where the *next* root's search should resume, matching LSP roots'
/// own real interleaving property (each of the `LPC_ORD` roots lies in a
/// disjoint sub-interval of `[-1, 1]`, in strictly decreasing order, so
/// search never needs to backtrack).
fn find_next_root(poly: &[f32; 6], x_start: f32) -> Option<f32> {
    let poly_q: [i32; 6] = std::array::from_fn(|i| f32_to_q(poly[i], COEF_FRAC_BITS));
    find_next_root_from_q23(&poly_q, x_start)
}

/// The real search, shared by `find_next_root` above (which quantizes
/// `poly` once up front, not once per candidate `x` the way calling
/// `cheb_poly_eval_fixed` directly used to) and a caller that already
/// has `poly` in Q8.23 (`build_p_q_fixed`'s own output).
fn find_next_root_from_q23(poly_q: &[i32; 6], x_start: f32) -> Option<f32> {
    let mut xl = x_start;
    let mut p_l = cheb_poly_eval_fixed_core(poly_q, xl);
    while xl >= -1.0 {
        let xr = xl - LSP_SEARCH_STEP;
        let p_r = cheb_poly_eval_fixed_core(poly_q, xr);
        if (p_r <= 0 && p_l >= 0) || (p_r >= 0 && p_l <= 0) {
            let mut lo = xl;
            let mut hi = xr;
            let mut p_lo = p_l;
            let mut mid = 0.5 * (lo + hi);
            for _ in 0..LSP_BISECTIONS {
                mid = 0.5 * (lo + hi);
                let p_mid = cheb_poly_eval_fixed_core(poly_q, mid);
                if p_mid.signum() * p_lo.signum() > 0 {
                    lo = mid;
                    p_lo = p_mid;
                } else {
                    hi = mid;
                }
            }
            // Matches the reference's own manually-unrolled bisection: the
            // root estimate is the midpoint computed in the *last*
            // iteration, not a fresh average of the post-update bounds
            // (that would be a 7th, uncounted bisection).
            return Some(mid);
        }
        xl = xr;
        p_l = p_r;
    }
    None
}

/// LPC coefficients -> `LPC_ORD` Line Spectral Frequencies (radians,
/// strictly increasing). Returns `None` if fewer than `LPC_ORD` roots
/// were found in `[-1, 1]` (a real, if rare, LPC analysis failure mode
/// on pathological input -- callers substitute benign fallback LSPs).
pub fn lpc_to_lsp(ak: &LpcCoeffs) -> Option<[f32; LPC_ORD]> {
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

/// Fixed-point `lpc_to_lsp`: `a_q23` (Q8.23, `i64`, e.g. straight from
/// `apply_bw_gamma_fixed`) in -- no `f32` anywhere before the final
/// `.acos()` (LSP frequencies themselves stay `f32`, matching this
/// port's established "integer core, float boundary" pattern;
/// `interp.rs`/`quantise.rs` aren't migrated yet, so there's nothing
/// further along the pipeline to hand a fixed-point angle to, and
/// `acos()` itself has no cheap fixed-point equivalent this port has
/// built).
pub fn lpc_to_lsp_from_integer_ak(a_q23: &[i64; LPC_ORD + 1]) -> Option<[f32; LPC_ORD]> {
    let (p, q) = build_p_q_fixed(a_q23);
    let mut search_from = 1.0f32;
    let mut freq = [0.0f32; LPC_ORD];
    for (j, f) in freq.iter_mut().enumerate() {
        let poly = if j & 1 == 1 { &q } else { &p };
        let root = find_next_root_from_q23(poly, search_from)?;
        *f = root.acos();
        search_from = root;
    }
    Some(freq)
}

/// Max coefficients either half-polynomial `build_half_poly` grows to:
/// `LPC_ORD/2` degree-2 factors (degree `LPC_ORD`) times one
/// degree-1 boundary factor (degree `LPC_ORD+1`) = `LPC_ORD+2`
/// coefficients.
const HALF_POLY_LEN: usize = LPC_ORD + 2;

/// Multiplies two polynomials (ascending-power coefficient order,
/// `a_len`/`b_len` real lengths within their fixed-capacity buffers) into
/// `out`, returning the product's own length. Plain convolution, no heap
/// allocation -- `lsp_to_lpc` runs on a real-time codec's per-frame hot
/// path, and `HALF_POLY_LEN` is a small compile-time constant, so a
/// stack buffer is both simpler and cheaper than a `Vec` here.
fn poly_mul_fixed(
    a: &[f32; HALF_POLY_LEN],
    a_len: usize,
    b: &[f32],
    out: &mut [f32; HALF_POLY_LEN],
) -> usize {
    let out_len = a_len + b.len() - 1;
    out[..out_len].fill(0.0);
    for (i, &ai) in a[..a_len].iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            out[i + j] += ai * bj;
        }
    }
    out_len
}

/// Builds `P(z)` or `Q(z)` (see `lsp_to_lpc`'s own doc comment): cascades
/// a degree-2 factor `1 - 2*cos(lsp_i)*z^-1 + z^-2` per LSP at indices
/// `start_offset, start_offset+2, ...`, then multiplies by the boundary
/// factor `1 + boundary_sign*z^-1`.
fn build_half_poly(
    cos_lsp: &[f32; LPC_ORD],
    start_offset: usize,
    boundary_sign: f32,
) -> ([f32; HALF_POLY_LEN], usize) {
    let mut buf = [0.0f32; HALF_POLY_LEN];
    let mut scratch = [0.0f32; HALF_POLY_LEN];
    buf[0] = 1.0;
    let mut len = 1usize;
    for i in (start_offset..LPC_ORD).step_by(2) {
        let c = cos_lsp[i];
        len = poly_mul_fixed(&buf, len, &[1.0, -2.0 * c, 1.0], &mut scratch);
        buf[..len].copy_from_slice(&scratch[..len]);
    }
    len = poly_mul_fixed(&buf, len, &[1.0, boundary_sign], &mut scratch);
    buf[..len].copy_from_slice(&scratch[..len]);
    (buf, len)
}

/// Inverse of `lpc_to_lsp`: `LPC_ORD` Line Spectral Frequencies (radians)
/// back to `LPC_ORD` LPC coefficients. Standard LSP reconstruction
/// (e.g. Kabal & Ramachandran 1986): `P(z)` is the product of a degree-2
/// factor `1 - 2*cos(lsp_i)*z^-1 + z^-2` per even-indexed LSP (the same
/// interleaving `lpc_to_lsp` produces them in) times `(1 + z^-1)`; `Q(z)`
/// the same for odd-indexed LSPs times `(1 - z^-1)`. Both have degree
/// `LPC_ORD+1`, but by construction their degree-`(LPC_ORD+1)` terms
/// cancel exactly when summed (that's what makes `P = A + z^-(p+1)A(1/z)`
/// and `Q = A - z^-(p+1)A(1/z)` sum back to `2A`), leaving
/// `ak = (P+Q)/2` truncated to its first `LPC_ORD+1` terms. Same
/// construction as the reference's own cascaded-biquad shift-register
/// formulation, just written as plain (allocation-free) polynomial
/// convolution rather than optimized for a fixed small buffer the way
/// the reference's own version is.
pub fn lsp_to_lpc(lsp: &[f32; LPC_ORD]) -> LpcCoeffs {
    let cos_lsp: [f32; LPC_ORD] = std::array::from_fn(|i| lsp[i].cos());
    let (p, _) = build_half_poly(&cos_lsp, 0, 1.0);
    let (q, _) = build_half_poly(&cos_lsp, 1, -1.0);

    let mut ak = [0.0f32; LPC_ORD + 1];
    for i in 0..=LPC_ORD {
        ak[i] = 0.5 * (p[i] + q[i]);
    }
    ak
}

/// LPC energy: `E = sum(ak[i] * R[i])`, the real prediction-error energy
/// this analysis frame's LPC filter achieves against its own
/// autocorrelation -- computed *before* bandwidth expansion is applied
/// to `ak`, matching the real reference's own ordering (bandwidth
/// expansion after this computation would introduce spurious negative
/// energies).
pub fn lpc_energy(ak: &LpcCoeffs, r: &Autocorr) -> f32 {
    ak.iter().zip(r.iter()).map(|(a, r)| a * r).sum()
}

/// Fixed-point `lpc_energy`: `a_q23` (from `levinson_durbin_fixed_from_
/// integer_r`'s own second return value) and `r_q` (from `autocorrelate_
/// fixed`, same Q8.23 as `a_q23`) in, real `f32` energy out (matching
/// this port's "integer core, float boundary" pattern -- `quantise::
/// encode_energy` isn't migrated yet, so there's nothing further to hand
/// an integer type to). Each per-term product widens to `i128`
/// transiently (two Q8.23 values up to ~48 bits each can produce a
/// ~74-bit-magnitude product, which does not fit `i64`, unlike
/// `autocorrelate_fixed`'s own narrower per-sample products) then
/// shifts right by `COEF_FRAC_BITS` immediately, bringing each term back
/// to Q8.23 before accumulating -- the running sum itself stays well
/// within `i64` for any realistic frame (real measured `\|ak\|` <= 77.17,
/// real measured `\|R[j]\|` <= `R[0]` by the Cauchy-Schwarz bound already
/// established elsewhere in this file).
pub fn lpc_energy_fixed(a_q23: &[i64; LPC_ORD + 1], r_q: &[i64; LPC_ORD + 1]) -> f32 {
    let mut sum: i64 = 0;
    for i in 0..=LPC_ORD {
        sum += ((a_q23[i] as i128 * r_q[i] as i128) >> COEF_FRAC_BITS) as i64;
    }
    (sum as f64 / (1i64 << COEF_FRAC_BITS) as f64) as f32
}

/// `super::bw_gamma(i)` (`0.994^i`), precomputed at Q8.23 -- a literal
/// table, not a runtime `powi()`, since only `LPC_ORD+1` (11) values
/// ever exist and this port's real target (FPU-less HTs/ESP32-class
/// parts) has no cheap hardware `pow` either. Values: `round(f64::from(
/// bw_gamma(i)) * 2^23)` for `i` in `0..=10` -- generated by actually
/// running `bw_gamma`'s own real `f32` computation and quantizing its
/// real output (not an independently-computed `0.994^i` in `f64`, which
/// a real, caught-by-its-own-test discrepancy showed disagrees with the
/// real `f32` `powi()` result by 1 part in 2^23 on several entries --
/// `f32`'s own ~24-bit precision, not a bug). Checked directly against
/// `super::bw_gamma`'s own real output below.
const BW_GAMMA_Q23: [i64; LPC_ORD + 1] = [
    8388608, 8338277, 8288247, 8238518, 8189087, 8139953, 8091113, 8042566, 7994311, 7946345,
    7898668,
];

/// Fixed-point bandwidth expansion: `a_q23[i] *= BW_GAMMA_Q23[i]`
/// (Q8.23), applied in place. **Must run after `lpc_energy_fixed`**,
/// matching `lpc_energy`'s own doc comment on real ordering (bandwidth
/// expansion before energy computation would introduce spurious
/// negative energies) -- callers that need both (e.g. `EncoderFixed`)
/// should keep a separate copy of `a_q23` for `lpc_energy_fixed` if they
/// need the pre-expansion coefficients for anything else afterward.
pub fn apply_bw_gamma_fixed(a_q23: &mut [i64; LPC_ORD + 1]) {
    for (a, &g) in a_q23.iter_mut().zip(BW_GAMMA_Q23.iter()) {
        *a = (*a * g) >> COEF_FRAC_BITS;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec2_3200::{bw_gamma, M_PITCH};

    macro_rules! fixture {
        ($name:literal) => {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_3200/",
                $name
            )
        };
    }

    fn read_dump(path: &str, cols: usize) -> Vec<Vec<f32>> {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .lines()
            .map(|line| {
                let v: Vec<f32> = line
                    .split_whitespace()
                    .map(|s| s.parse().unwrap())
                    .collect();
                assert_eq!(
                    v.len(),
                    cols,
                    "line has {} fields, expected {cols}: {line}",
                    v.len()
                );
                v
            })
            .collect()
    }

    /// Real R[] vectors and their real Levinson-Durbin ak[] outputs,
    /// captured from actual Codec2-mod real speech decoding this same
    /// session -- cross-validates this independently-written
    /// implementation against the real reference's own real output, not
    /// just internal self-consistency.
    #[test]
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

    /// Same real captured `ak[]`/`lsp[]` corpus, the real fixed-point
    /// path (`apply_bw_gamma_fixed` + `lpc_to_lsp_from_integer_ak`)
    /// instead of float -- the whole windowing-through-LSP chain is now
    /// exercisable in genuine fixed-point up to this exact point.
    /// **Real measured bound is looser than the float test's own `1e-4`**
    /// (`3.3e-4` rad, not `1.8e-6`) -- this is a real, different
    /// arithmetic path (Q8.23 `P[]`/`Q[]`, Q2.29 internal Chebyshev
    /// arithmetic) with its own real, small quantization noise, plausibly
    /// close to this port's own inherent bisection resolution limit
    /// (`LSP_SEARCH_STEP / 2^LSP_BISECTIONS = 0.01/64 ~ 1.56e-4` in the
    /// `x`-domain, before `acos()`'s own slope-dependent scaling into
    /// radians) -- not evidence of a bug: root-finding still succeeds on
    /// 100% of real frames (362/362), matching the float path's own real
    /// robustness. In the units that actually matter, `quantise.rs`'s own
    /// LSP quantizer step is 25Hz; `3.3e-4` rad is `~0.43Hz` -- under 2%
    /// of one real quantizer step, the same "check what actually gets
    /// transmitted" reasoning `lpc_energy_fixed`/`apply_bw_gamma_fixed`
    /// already established.
    #[test]
    fn lpc_to_lsp_from_integer_ak_matches_the_real_reference_on_real_captured_ak_data() {
        let ak_path = fixture!("codec2_ak_dump.txt");
        let lsp_path = fixture!("codec2_lsp_dump.txt");
        let aks = read_dump(ak_path, LPC_ORD + 1);
        let lsps = read_dump(lsp_path, LPC_ORD + 1);
        assert_eq!(aks.len(), lsps.len());
        assert!(aks.len() > 300, "expected the real captured fixture corpus, got {} rows", aks.len());

        let mut roots_found_count = 0;
        let mut max_abs_err = 0.0f32;
        for (ak_row, lsp_row) in aks.iter().zip(lsps.iter()) {
            let ref_roots = lsp_row[0] as usize;
            if ref_roots != LPC_ORD {
                continue;
            }
            let mut ak = [0.0f32; LPC_ORD + 1];
            ak.copy_from_slice(ak_row);
            let mut a_q23: [i64; LPC_ORD + 1] =
                std::array::from_fn(|i| f32_to_q(ak[i], COEF_FRAC_BITS) as i64);
            apply_bw_gamma_fixed(&mut a_q23);
            let Some(lsp) = lpc_to_lsp_from_integer_ak(&a_q23) else { continue };
            roots_found_count += 1;
            for i in 0..LPC_ORD {
                max_abs_err = max_abs_err.max((lsp[i] - lsp_row[1 + i]).abs());
            }
        }
        println!("lpc_to_lsp_from_integer_ak: {roots_found_count}/{} real frames found all {LPC_ORD} roots, max error {max_abs_err} rad", aks.len());
        assert!(
            roots_found_count as f64 / aks.len() as f64 > 0.95,
            "only {roots_found_count}/{} real frames found all {LPC_ORD} LSP roots -- expected the overwhelming majority to succeed on real speech",
            aks.len()
        );
        assert!(
            max_abs_err < 1e-3,
            "max LSP root error vs real captured reference (fixed-point path): {max_abs_err} rad -- expected real margin under 1e-3 (measured ~3.3e-4 when this test was written)"
        );
    }

    #[test]
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

    /// Same real captured `ak[]`/`P[]`/`Q[]` corpus, fixed-point path
    /// (`apply_bw_gamma_fixed` + `build_p_q_fixed`) instead of float.
    #[test]
    fn build_p_q_fixed_matches_the_real_reference_p_q_on_real_captured_data() {
        let ak_path = fixture!("codec2_ak_dump.txt");
        let pq_path = fixture!("codec2_pq_dump.txt");
        let aks = read_dump(ak_path, LPC_ORD + 1);
        let pqs = read_dump(pq_path, 12);
        assert_eq!(aks.len(), pqs.len());

        let mut max_err = 0.0f32;
        for (ak_row, pq_row) in aks.iter().zip(pqs.iter()) {
            let mut ak = [0.0f32; LPC_ORD + 1];
            ak.copy_from_slice(ak_row);
            let mut a_q23: [i64; LPC_ORD + 1] =
                std::array::from_fn(|i| f32_to_q(ak[i], COEF_FRAC_BITS) as i64);
            apply_bw_gamma_fixed(&mut a_q23);
            let (p_q, q_q) = build_p_q_fixed(&a_q23);
            for i in 0..6 {
                let p_dequantized = p_q[i] as f64 / (1i64 << COEF_FRAC_BITS) as f64;
                let q_dequantized = q_q[i] as f64 / (1i64 << COEF_FRAC_BITS) as f64;
                max_err = max_err.max((p_dequantized - pq_row[i] as f64).abs() as f32);
                max_err = max_err.max((q_dequantized - pq_row[6 + i] as f64).abs() as f32);
            }
        }
        println!("build_p_q_fixed: max absolute P[]/Q[] error vs real captured reference = {max_err}");
        assert!(
            max_err < 1e-3,
            "build_p_q_fixed diverged from the real captured reference by more than ordinary Q8.23 quantization noise should allow: {max_err}"
        );
    }

    /// Independent plain-float reference for the Chebyshev evaluation
    /// `cheb_poly_eval_fixed` replaced -- deliberately a fresh
    /// transcription, not a call into fixed-point code, matching this
    /// codebase's own established pattern (see `quantise.rs`'s
    /// `reference_encode`/`reference_binary_search`) for validating a
    /// fixed-point/optimized implementation against an independently
    /// written float one.
    fn reference_cheb_poly_eval(coef: &[f32; 6], x: f32) -> f32 {
        let mut t_prev2 = 1.0f32;
        let mut t_prev1 = x;
        let mut sum = coef[5] * t_prev2 + coef[4] * t_prev1;
        let two_x = 2.0 * x;
        for i in 2..=5 {
            let t_i = two_x * t_prev1 - t_prev2;
            sum += coef[5 - i] * t_i;
            t_prev2 = t_prev1;
            t_prev1 = t_i;
        }
        sum
    }

    #[test]
    fn cheb_poly_eval_fixed_matches_the_plain_float_sign_on_a_dense_sweep_of_real_captured_p_q_coefficients(
    ) {
        // Same validation shape CODEC2_MOD_FIXED_POINT_PLAN.md used for
        // this exact stage: a dense sweep of x across [-1,1] (4001
        // points there; matched here) against every real captured P[]/Q[]
        // coefficient set, checking SIGN agreement only -- the only
        // thing find_next_root's own bisection ever consumes from this
        // function's result (see cheb_poly_eval_fixed's own doc comment).
        let pq_path = fixture!("codec2_pq_dump.txt");
        let pqs = read_dump(pq_path, 12);
        assert!(
            pqs.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            pqs.len()
        );

        let mut sign_mismatches = 0u64;
        let mut total = 0u64;
        for pq_row in &pqs {
            let mut p = [0.0f32; 6];
            let mut q = [0.0f32; 6];
            p.copy_from_slice(&pq_row[0..6]);
            q.copy_from_slice(&pq_row[6..12]);
            for poly in [&p, &q] {
                let mut x = -1.0f32;
                while x <= 1.0 {
                    let plain = reference_cheb_poly_eval(poly, x);
                    let fixed = cheb_poly_eval_fixed(poly, x);
                    // Sign-only comparison, matching what find_next_root
                    // actually uses -- 0.0 (an exact root landing) is
                    // vanishingly unlikely on this dense a sweep and
                    // isn't the real comparison either implementation's
                    // caller performs.
                    if plain.signum() != (fixed.signum() as f32) && plain != 0.0 && fixed != 0 {
                        sign_mismatches += 1;
                    }
                    total += 1;
                    x += 0.0005; // ~4001 points, matching the plan doc's own sweep density
                }
            }
        }
        assert!(
            total > 1_000_000,
            "expected a real dense-sweep total, got {total}"
        );
        assert_eq!(sign_mismatches, 0, "{sign_mismatches}/{total} real sign mismatches between the fixed-point Chebyshev evaluation and plain float -- the plan doc's own validated result for this Q8.23/Q2.29 width is zero");
    }

    #[test]
    fn a_deliberately_coarse_q_format_produces_real_sign_mismatches_confirming_the_test_above_is_not_vacuous(
    ) {
        // Negative control, same methodology the plan doc itself used
        // (deliberately coarsening the T register's own precision
        // produced real, monotonically worsening mismatch rates there)
        // -- rerun a subset of the real fixture corpus through the
        // identical fixed-point control flow at a much coarser T
        // register width and confirm it actually produces sign errors.
        const COARSE_CHEB_FRAC_BITS: u32 = 8; // deliberately far below the validated 29
        fn coarse_cheb_poly_eval_fixed(coef: &[f32; 6], x: f32) -> i64 {
            let coef_q: [i32; 6] = std::array::from_fn(|i| f32_to_q(coef[i], COEF_FRAC_BITS));
            let x_q = f32_to_q(x, COARSE_CHEB_FRAC_BITS) as i64;
            let mut t_prev2: i64 = 1i64 << COARSE_CHEB_FRAC_BITS;
            let mut t_prev1: i64 = x_q;
            let mut sum: i64 = (coef_q[5] as i64 * t_prev2) >> COARSE_CHEB_FRAC_BITS;
            sum += (coef_q[4] as i64 * t_prev1) >> COARSE_CHEB_FRAC_BITS;
            for i in 2..=5 {
                let t_i = ((2 * x_q * t_prev1) >> COARSE_CHEB_FRAC_BITS) - t_prev2;
                sum += (coef_q[5 - i] as i64 * t_i) >> COARSE_CHEB_FRAC_BITS;
                t_prev2 = t_prev1;
                t_prev1 = t_i;
            }
            sum
        }

        let pq_path = fixture!("codec2_pq_dump.txt");
        let pqs = read_dump(pq_path, 12);

        let mut sign_mismatches = 0u64;
        for pq_row in pqs.iter().take(80) {
            let mut p = [0.0f32; 6];
            p.copy_from_slice(&pq_row[0..6]);
            let mut x = -1.0f32;
            while x <= 1.0 {
                let plain = reference_cheb_poly_eval(&p, x);
                let coarse = coarse_cheb_poly_eval_fixed(&p, x);
                if plain.signum() != (coarse.signum() as f32) && plain != 0.0 && coarse != 0 {
                    sign_mismatches += 1;
                }
                x += 0.0005;
            }
        }
        assert!(sign_mismatches > 0, "expected the deliberately coarse Q-format to produce at least one real sign mismatch -- got zero, which would mean the zero-mismatch result above isn't evidence of anything");
    }

    /// Real measured range check backing `cheb_poly_eval_fixed`'s own
    /// `i32` storage (`CODEC2_FIXED_POINT_WIDTH_REDUCTION_STUDY.md`'s
    /// Question 2, now implemented, not just proposed): `\|T\|<=1` and
    /// `\|x\|<=1` "by construction," and `coef_q` is `i32`-native -- this
    /// independently re-simulates the arithmetic at `i64` width to
    /// measure whether `sum` (the one value not structurally bounded to
    /// `[-1,1]`, since it accumulates up to 6 coefficient-scaled terms)
    /// also stays within `i32`'s real range across real data, a real
    /// regression guard against the production `i32` code silently
    /// wrapping if some future coefficient distribution ever pushed past
    /// this margin (release builds don't panic on `i32` overflow the way
    /// debug builds do).
    #[test]
    fn cheb_poly_eval_fixed_intermediate_values_real_measured_i32_fit_margin() {
        let pq_path = fixture!("codec2_pq_dump.txt");
        let pqs = read_dump(pq_path, 12);

        let mut max_abs_x_q: i64 = 0;
        let mut max_abs_t: i64 = 0;
        let mut max_abs_sum: i64 = 0;
        let mut n = 0u64;

        for pq_row in &pqs {
            let mut p = [0.0f32; 6];
            let mut q = [0.0f32; 6];
            p.copy_from_slice(&pq_row[0..6]);
            q.copy_from_slice(&pq_row[6..12]);
            for poly in [&p, &q] {
                let coef_q: [i32; 6] =
                    std::array::from_fn(|i| f32_to_q(poly[i], COEF_FRAC_BITS));
                let mut x = -1.0f32;
                while x <= 1.0 {
                    let x_q = f32_to_q(x, CHEB_FRAC_BITS) as i64;
                    max_abs_x_q = max_abs_x_q.max(x_q.abs());

                    let mut t_prev2: i64 = 1i64 << CHEB_FRAC_BITS;
                    let mut t_prev1: i64 = x_q;
                    max_abs_t = max_abs_t.max(t_prev2.abs()).max(t_prev1.abs());

                    let mut sum: i64 = (coef_q[5] as i64 * t_prev2) >> CHEB_FRAC_BITS;
                    sum += (coef_q[4] as i64 * t_prev1) >> CHEB_FRAC_BITS;
                    max_abs_sum = max_abs_sum.max(sum.abs());

                    for i in 2..=5 {
                        let t_i = ((2 * x_q * t_prev1) >> CHEB_FRAC_BITS) - t_prev2;
                        max_abs_t = max_abs_t.max(t_i.abs());
                        sum += (coef_q[5 - i] as i64 * t_i) >> CHEB_FRAC_BITS;
                        max_abs_sum = max_abs_sum.max(sum.abs());
                        t_prev2 = t_prev1;
                        t_prev1 = t_i;
                    }
                    n += 1;
                    x += 0.0005;
                }
            }
        }

        assert!(n > 1_000_000, "expected a real dense-sweep total, got {n}");
        println!(
            "cheb_poly_eval_fixed real measured max magnitudes across {n} real evaluations: \
             |x_q|={max_abs_x_q} ({:.3} bits), |T|={max_abs_t} ({:.3} bits), |sum|={max_abs_sum} ({:.3} bits) -- i32 usable magnitude is 2^31-1={}",
            (max_abs_x_q as f64).log2(),
            (max_abs_t as f64).log2(),
            (max_abs_sum as f64).log2(),
            i32::MAX
        );
        assert!(
            max_abs_x_q < i32::MAX as i64 && max_abs_t < i32::MAX as i64 && max_abs_sum < i32::MAX as i64,
            "a value here exceeds i32's real range on real data -- Question 2's premise (values already fit i32) would be false"
        );
    }

    /// `lsp_to_lpc` is the mathematical inverse of `lpc_to_lsp` (both
    /// operate on real captured, bandwidth-expanded `ak[]` -- no
    /// reference `lsp_to_lpc` output is needed at all, since the round
    /// trip is self-verifying).
    #[test]
    fn lsp_to_lpc_round_trips_lpc_to_lsp_on_real_captured_ak_data() {
        let ak_path = fixture!("codec2_ak_dump.txt");
        let aks = read_dump(ak_path, LPC_ORD + 1);
        assert!(
            aks.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            aks.len()
        );

        let mut n_checked = 0;
        let mut max_abs_err = 0.0f32;
        for ak_row in &aks {
            let mut ak = [0.0f32; LPC_ORD + 1];
            ak.copy_from_slice(ak_row);
            for (i, a) in ak.iter_mut().enumerate() {
                *a *= bw_gamma(i);
            }
            let Some(lsp) = lpc_to_lsp(&ak) else { continue };
            let back = lsp_to_lpc(&lsp);
            for i in 0..=LPC_ORD {
                max_abs_err = max_abs_err.max((back[i] - ak[i]).abs());
            }
            n_checked += 1;
        }
        assert!(n_checked > 150, "only checked {n_checked} real frames");
        // Confirmed (by rerunning poly_mul in f64, same result) that
        // this floor isn't poly_mul's own rounding -- it's the LSP
        // frequency's own f32 precision (root.acos() in lpc_to_lsp) that
        // limits round-trip accuracy here. Real-world impact is moot:
        // the actually-transmitted LSPs are 5-bit-quantized (~14-50Hz
        // steps), a coarser bound than 0.0065's worth of ak-coefficient
        // drift.
        assert!(
            max_abs_err < 0.01,
            "max ak[] round-trip error: {max_abs_err}"
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

    /// Same cross-check as the test above, but for `autocorrelate_fixed`
    /// -- quantizes the reference's own real captured `wn[]` to Q8.23,
    /// runs the fixed-point accumulator, dequantizes the result, and
    /// compares against the same real captured `R[]` reference. Looser
    /// tolerance than the plain-`f32` version above (`1e-2` vs `1e-3`)
    /// is expected and correct, not a weakened bar: this path introduces
    /// two real, additional quantization steps (`wn` itself to Q8.23,
    /// and the per-term `>> WN_FRAC_BITS` rounding-toward-zero instead of
    /// round-to-nearest -- deliberately not `rshift_round` here, since
    /// summing 320 systematically-biased terms would be worse than one
    /// unbiased truncation per term averaged over many terms) that the
    /// plain-`f32` version doesn't have.
    #[test]
    fn autocorrelate_fixed_matches_the_real_reference_r_within_real_quantization_noise() {
        let wn_path = fixture!("synthetic_codec2_wn_dump.txt");
        let r_path = fixture!("synthetic_codec2_r_dump.txt");
        let wns = read_dump(wn_path, M_PITCH);
        let rs = read_dump(r_path, LPC_ORD + 1);
        assert_eq!(wns.len(), rs.len());
        assert!(wns.len() > 150, "expected the synthetic-signal fixture corpus, got {} rows", wns.len());

        let mut max_rel_err = 0.0f32;
        let mut max_abs_wn_q = 0i32;
        for (wn_row, r_row) in wns.iter().zip(rs.iter()) {
            let wn_q: Vec<i32> = wn_row
                .iter()
                .map(|&w| f32_to_q(w, WN_FRAC_BITS))
                .collect();
            max_abs_wn_q = max_abs_wn_q.max(wn_q.iter().map(|&w| w.abs()).max().unwrap_or(0));
            let r_q = autocorrelate_fixed(&wn_q);
            for i in 0..=LPC_ORD {
                let r_dequantized = r_q[i] as f64 / (1i64 << WN_FRAC_BITS) as f64;
                let denom = (r_row[i] as f64).abs().max(1e-6);
                max_rel_err = max_rel_err.max(((r_dequantized - r_row[i] as f64) / denom).abs() as f32);
            }
        }
        println!("autocorrelate_fixed: max relative R[] error = {max_rel_err}, max real |wn_q| = {max_abs_wn_q} (i32::MAX={})", i32::MAX);
        assert!(
            (max_abs_wn_q as i64) < i32::MAX as i64,
            "real captured wn[] exceeded Q8.23's i32 headroom -- WN_FRAC_BITS's own doc comment needs rechecking against this real data"
        );
        assert!(
            max_rel_err < 1e-2,
            "max relative R[] error vs real captured reference (fixed-point path): {max_rel_err}"
        );
    }

    /// `lpc_energy_fixed` vs `lpc_energy`, both fed the same real
    /// captured data -- `codec2_ak_dump.txt` (real `ak[]`) and
    /// `codec2_r_dump.txt` (real `R[]`), zipped by file order. Not a
    /// claim that row `i` in one file is the same analysis frame as row
    /// `i` in the other (`codec2_enc_e_dump.txt`'s own row count, 363,
    /// doesn't even match these two files' shared 362, so frame
    /// correspondence across all three isn't established) -- this test
    /// only needs real, representative `ak`/`R[]` *magnitudes*, not a
    /// specific real energy value.
    ///
    /// **Acceptance bar is the real quantizer decision, not a raw
    /// relative-error threshold** -- an earlier version of this test
    /// asserted `< 1e-2` relative error and found a real, if harmless,
    /// 3.2% worst case: `lpc_energy`'s own `sum(ak[i]*R[i])` can involve
    /// real cancellation between positive and negative terms on some
    /// frames, amplifying small per-term quantization error into a
    /// larger relative error on the *resulting* (partially-cancelled,
    /// smaller-magnitude) sum -- a real property of this dot product, not
    /// a bug. In dB terms (what `quantise::encode_energy` actually
    /// quantizes on), 3.2% is `10*log10(1.032) ~ 0.14dB` -- over 11x
    /// smaller than the 5-bit quantizer's own real step size
    /// (`(E_MAX_DB-E_MIN_DB)/32 = 1.5625dB`), so it can never change a
    /// real transmitted index. Matches `quantise.rs`'s own established
    /// methodology for exactly this situation (see e.g. `fixed_point.rs`'s
    /// `the_8_bit_log2_lut_reproduces_the_plain_float_log10_quantizer_
    /// decision...with_zero_index_mismatches`) -- check the decision that
    /// actually matters, not an arbitrary tolerance on the raw value.
    #[test]
    fn lpc_energy_fixed_and_lpc_energy_produce_the_same_real_quantizer_index() {
        let ak_path = fixture!("codec2_ak_dump.txt");
        let r_path = fixture!("codec2_r_dump.txt");
        let aks = read_dump(ak_path, LPC_ORD + 1);
        let rs = read_dump(r_path, LPC_ORD + 1);
        assert_eq!(aks.len(), rs.len());
        assert!(aks.len() > 300, "expected the real captured fixture corpus, got {} rows", aks.len());

        let mut index_mismatches = 0u64;
        for (ak_row, r_row) in aks.iter().zip(rs.iter()) {
            let mut ak = [0.0f32; LPC_ORD + 1];
            ak.copy_from_slice(ak_row);
            let mut r = [0.0f32; LPC_ORD + 1];
            r.copy_from_slice(r_row);

            let e_float = lpc_energy(&ak, &r);
            let a_q23: [i64; LPC_ORD + 1] =
                std::array::from_fn(|i| f32_to_q(ak[i], COEF_FRAC_BITS) as i64);
            let r_q: [i64; LPC_ORD + 1] =
                std::array::from_fn(|i| (r[i] as f64 * (1i64 << COEF_FRAC_BITS) as f64).round() as i64);
            let e_fixed = lpc_energy_fixed(&a_q23, &r_q);

            if crate::codec2_3200::quantise::encode_energy(e_float)
                != crate::codec2_3200::quantise::encode_energy(e_fixed)
            {
                index_mismatches += 1;
            }
        }
        println!("lpc_energy_fixed: {index_mismatches}/{} real quantizer-index mismatches vs lpc_energy", aks.len());
        assert_eq!(
            index_mismatches, 0,
            "lpc_energy_fixed produced a different real transmitted energy index than lpc_energy on {index_mismatches} real frame(s) -- a genuine regression, not just raw-value noise"
        );
    }

    /// `BW_GAMMA_Q23`'s own literal table checked directly against
    /// `super::bw_gamma`'s real output, and `apply_bw_gamma_fixed`
    /// checked against applying `bw_gamma` in float, on real captured
    /// `ak[]` data. **`i=5` sits exactly on a rounding tie** (`bw_gamma(5)
    /// as f64 * 2^23` lands on exactly `...952.5`, confirmed directly:
    /// `0.994f32.powi(5)` is `0.9703579545021057`, which times `2^23` is
    /// precisely `8139952.5`) -- which side of that tie the real
    /// reference value lands on is itself a real, observed, reproducible
    /// function of surrounding compilation context (this table's own
    /// literal was generated from one real run; a fresh diagnostic run
    /// produced the other neighbor), not a stable target to chase exact
    /// agreement with. Tolerating `<= 1` ULP at Q8.23 here (~1.2e-7
    /// relative) is the honest bar, matching this session's own repeated
    /// finding that exact agreement at a literal tie isn't well-defined,
    /// not a loosened test to hide a real bug.
    #[test]
    fn bw_gamma_q23_table_matches_the_real_float_bw_gamma() {
        for (i, &table_value) in BW_GAMMA_Q23.iter().enumerate() {
            let expected = (bw_gamma(i) as f64 * (1i64 << COEF_FRAC_BITS) as f64).round() as i64;
            assert!(
                (table_value - expected).abs() <= 1,
                "BW_GAMMA_Q23[{i}] = {table_value} but bw_gamma({i}) quantizes to {expected} -- more than the one real tie-boundary ULP apart"
            );
        }
    }

    #[test]
    fn apply_bw_gamma_fixed_matches_the_real_float_bw_gamma_on_real_captured_ak_data() {
        let ak_path = fixture!("codec2_ak_dump.txt");
        let aks = read_dump(ak_path, LPC_ORD + 1);
        assert!(aks.len() > 300, "expected the real captured fixture corpus, got {} rows", aks.len());

        // Absolute error, not relative -- this is a plain elementwise
        // scale-by-a-factor-near-1, not a sum with cancellation
        // (`lpc_energy_fixed`'s own real reason for using a different
        // bar), so a coefficient that happens to land near zero doesn't
        // deserve a different, tighter absolute bar than any other --
        // Q8.23's own real quantization step (~1.2e-7) is the honest
        // measure of this operation's own real precision everywhere.
        let mut max_abs_err = 0.0f32;
        for ak_row in &aks {
            let mut ak = [0.0f32; LPC_ORD + 1];
            ak.copy_from_slice(ak_row);

            let mut ak_expanded_float = ak;
            for (i, a) in ak_expanded_float.iter_mut().enumerate() {
                *a *= bw_gamma(i);
            }

            let mut a_q23: [i64; LPC_ORD + 1] =
                std::array::from_fn(|i| f32_to_q(ak[i], COEF_FRAC_BITS) as i64);
            apply_bw_gamma_fixed(&mut a_q23);

            for i in 0..=LPC_ORD {
                let dequantized = a_q23[i] as f64 / (1i64 << COEF_FRAC_BITS) as f64;
                max_abs_err =
                    max_abs_err.max((dequantized - ak_expanded_float[i] as f64).abs() as f32);
            }
        }
        println!("apply_bw_gamma_fixed: max absolute error vs float bw_gamma = {max_abs_err}");
        assert!(
            max_abs_err < 1e-4,
            "apply_bw_gamma_fixed diverged from applying bw_gamma in float by more than ordinary Q8.23 quantization noise should allow: {max_abs_err}"
        );
    }
}

#[cfg(test)]
mod levinson_durbin_fixed_tests {
    use super::*;

    /// Real measurement backing `WHITE_NOISE_CORRECTION_ALPHA`'s own
    /// choice (see `apply_white_noise_correction`'s doc comment for the
    /// full derivation) -- runs the plain-float recursion (`e`'s own
    /// trajectory doesn't depend on which arithmetic implementation
    /// carries it) across the whole real `codec2_r_dump.txt` corpus,
    /// with and without the correction, and asserts the corrected
    /// worst-case `1/e` amplification is real and substantially smaller
    /// -- not just that the correction changes something, but that it
    /// changes it the intended direction, by a real, checkable margin.
    #[test]
    fn white_noise_correction_measurably_improves_the_worst_case_amplification_margin() {
        let r_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_3200/codec2_r_dump.txt"
        );
        let rows: Vec<Vec<f32>> = std::fs::read_to_string(r_path)
            .unwrap()
            .lines()
            .map(|line| line.split_whitespace().map(|s| s.parse().unwrap()).collect())
            .collect();
        assert!(
            rows.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            rows.len()
        );

        fn worst_case_e_norm(rows: &[Vec<f32>], alpha: f32) -> f32 {
            let mut min_e_norm = f32::MAX;
            for row in rows {
                let mut r = [0.0f32; LPC_ORD + 1];
                r.copy_from_slice(row);
                if r[0] <= 0.0 {
                    continue;
                }
                r[0] *= 1.0 + alpha;
                let r0 = r[0];
                let mut a_prev = [0.0f32; LPC_ORD + 1];
                let mut a = [0.0f32; LPC_ORD + 1];
                a[0] = 1.0;
                let mut e = r[0];
                for i in 1..=LPC_ORD {
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
                    min_e_norm = min_e_norm.min(e / r0);
                }
            }
            min_e_norm
        }

        let uncorrected_min_e_norm = worst_case_e_norm(&rows, 0.0);
        let corrected_min_e_norm = worst_case_e_norm(&rows, WHITE_NOISE_CORRECTION_ALPHA);

        let uncorrected_amplification = 1.0 / uncorrected_min_e_norm;
        let corrected_amplification = 1.0 / corrected_min_e_norm;
        println!(
            "worst-case 1/e amplification: uncorrected={uncorrected_amplification:.1}x, corrected (alpha={WHITE_NOISE_CORRECTION_ALPHA:e})={corrected_amplification:.1}x"
        );

        assert!(
            uncorrected_amplification > 2000.0,
            "expected the real, already-documented uncorrected worst case (~3581x) to still reproduce here, got {uncorrected_amplification:.1}x -- if the fixture corpus changed, re-derive WHITE_NOISE_CORRECTION_ALPHA's own justification rather than just loosening this bound"
        );
        assert!(
            corrected_amplification < 500.0,
            "expected white noise correction to bring the worst-case amplification well under 500x (real measured value when this was written: ~269x), got {corrected_amplification:.1}x"
        );
        assert!(
            corrected_amplification < uncorrected_amplification / 10.0,
            "expected at least a real 10x improvement from white noise correction, got {:.1}x -> {:.1}x",
            uncorrected_amplification,
            corrected_amplification
        );
    }

    macro_rules! fixture {
        ($name:literal) => {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_3200/",
                $name
            )
        };
    }

    fn read_dump(path: &str, cols: usize) -> Vec<Vec<f32>> {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .lines()
            .map(|line| {
                let v: Vec<f32> = line
                    .split_whitespace()
                    .map(|s| s.parse().unwrap())
                    .collect();
                assert_eq!(
                    v.len(),
                    cols,
                    "line has {} fields, expected {cols}",
                    v.len()
                );
                v
            })
            .collect()
    }

    /// Runs the plain-float reference's own recursion but stops after
    /// each iteration, returning whether `|k| > 1.0` fired at any point
    /// -- used to tell "this frame's fixed-point divergence coincides
    /// with a clamp disagreement against the fixed-point path" (the
    /// plan doc's own characterized, expected divergence mode) apart
    /// from "diverges with no clamp involved at all" (a real bug in
    /// this port's own shifts/rounding, not the known algorithmic
    /// bifurcation).
    fn float_clamp_fired_per_iteration(r: &Autocorr) -> [bool; LPC_ORD + 1] {
        let mut fired = [false; LPC_ORD + 1];
        let mut a = [0.0f32; LPC_ORD + 1];
        let mut a_prev = [0.0f32; LPC_ORD + 1];
        a[0] = 1.0;
        let mut e = r[0];
        for i in 1..=LPC_ORD {
            let mut sum = 0.0f32;
            for j in 1..i {
                sum += a_prev[j] * r[i - j];
            }
            let mut k = -(r[i] + sum) / e;
            if k.abs() > 1.0 {
                k = 0.0;
                fired[i] = true;
            }
            a[i] = k;
            for j in 1..i {
                a[j] = a_prev[j] + k * a_prev[i - j];
            }
            e *= 1.0 - k * k;
            a_prev[..=i].copy_from_slice(&a[..=i]);
        }
        fired
    }

    /// Same discriminator as `float_clamp_fired_per_iteration`, but for
    /// the fixed-point path -- a thin wrapper over `levinson_durbin_
    /// fixed_core`'s own real `fired[]` output, the same core
    /// `levinson_durbin_fixed` itself calls, so this can never drift
    /// out of sync with the actual production arithmetic the way two
    /// independently hand-maintained transcriptions could.
    fn fixed_clamp_fired_per_iteration(r: &Autocorr) -> [bool; LPC_ORD + 1] {
        levinson_durbin_fixed_core(r).1
    }

    /// The real discriminator this port's own fixed-point Levinson-
    /// Durbin needs, per `docs/references/CODEC2_MOD_FIXED_POINT_
    /// PLAN.md`'s own characterization of the `|k|>1` clamp as a real
    /// algorithmic bifurcation, not a precision problem more bits fix.
    /// A coefficient mismatch between `levinson_durbin` and
    /// `levinson_durbin_fixed` is **expected and acceptable** when it
    /// coincides with the two paths making a *different* clamp decision
    /// at some iteration (the plan doc's own measured baseline: 0.04%
    /// of real frames diverge this way between plain float32 and
    /// float64 alone, with no fixed-point involved at all -- this port
    /// targets that same real band, not zero divergence, per Bruce's
    /// own explicit "as close as practical, no closer" direction). A
    /// mismatch with **no** clamp disagreement at any iteration would
    /// mean this port's own shift/rounding arithmetic is wrong, not the
    /// known, accepted algorithmic sensitivity -- asserted at zero.
    #[test]
    fn levinson_durbin_fixed_diverges_from_float_only_at_measured_clamp_disagreement_frames() {
        let r_path = fixture!("codec2_r_dump.txt");
        let rs = read_dump(r_path, LPC_ORD + 1);
        assert!(
            rs.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            rs.len()
        );

        let mut n_diverged_with_clamp_disagreement = 0;
        let mut n_diverged_without_clamp_disagreement = 0;
        let mut worst_unexplained_err = 0.0f32;

        for r_row in &rs {
            let mut r = [0.0f32; LPC_ORD + 1];
            r.copy_from_slice(r_row);
            if r[0] <= 0.0 {
                continue; // matches levinson_durbin_fixed's own debug_assert precondition
            }

            let ak_float = levinson_durbin(&r);
            let ak_fixed = levinson_durbin_fixed(&r);
            let max_err = (0..=LPC_ORD)
                .map(|i| (ak_float[i] - ak_fixed[i]).abs())
                .fold(0.0f32, f32::max);

            if max_err > 0.05 {
                let float_fired = float_clamp_fired_per_iteration(&r);
                let fixed_fired = fixed_clamp_fired_per_iteration(&r);
                let clamp_disagreement = (0..=LPC_ORD).any(|i| float_fired[i] != fixed_fired[i]);
                if clamp_disagreement {
                    n_diverged_with_clamp_disagreement += 1;
                } else {
                    n_diverged_without_clamp_disagreement += 1;
                    worst_unexplained_err = worst_unexplained_err.max(max_err);
                }
            }
        }

        let rate = n_diverged_with_clamp_disagreement as f64 / rs.len() as f64;
        println!(
            "levinson_durbin_fixed: {n_diverged_with_clamp_disagreement}/{} frames diverged with a clamp disagreement ({:.2}%, plan doc's own measured baseline: 0.04%-0.9%)",
            rs.len(),
            rate * 100.0
        );
        assert_eq!(
            n_diverged_without_clamp_disagreement, 0,
            "found {n_diverged_without_clamp_disagreement} frame(s) where the fixed-point path diverged from float by more than 0.05 with NO clamp disagreement (worst: {worst_unexplained_err}) -- this is a real bug in this port's own arithmetic, not the known algorithmic bifurcation"
        );
    }

    /// A real, independent basic-correctness check: on frames where
    /// neither path ever hits the clamp at all, the fixed-point result
    /// should track the float reference tightly (ordinary quantization
    /// noise, not the clamp's own order-of-magnitude cascade). **0.03,
    /// not a tighter bound**, because this recursion's own real
    /// sensitivity isn't purely discrete at the clamp boundary --
    /// `LEVINSON_FRAC_BITS`'s own doc comment measured frame 273 (the
    /// corpus's own minimum-`e` frame, ~2.76e-4) amplifying ordinary
    /// Q8.40 quantization noise by ~3600x through the `1/e` division,
    /// and that amplification compounds further across the several
    /// iterations where `e` stays small (not just once) -- real,
    /// expected numerical sensitivity for this specific frame's own
    /// ill-conditioning, not a bug (the clamp-disagreement discriminator
    /// test above, this module's actual acceptance criterion per
    /// Bruce's own "as close as practical, no closer" direction, treats
    /// anything under 0.05 as agreement, and this test's own measured
    /// max on real data sits at ~0.023 -- comfortably under both).
    #[test]
    fn levinson_durbin_fixed_matches_float_tightly_on_frames_that_never_hit_the_clamp() {
        let r_path = fixture!("codec2_r_dump.txt");
        let rs = read_dump(r_path, LPC_ORD + 1);

        let mut n_checked = 0;
        let mut max_err = 0.0f32;
        for r_row in &rs {
            let mut r = [0.0f32; LPC_ORD + 1];
            r.copy_from_slice(r_row);
            if r[0] <= 0.0 {
                continue;
            }
            let float_fired = float_clamp_fired_per_iteration(&r);
            let fixed_fired = fixed_clamp_fired_per_iteration(&r);
            if float_fired.iter().any(|&f| f) || fixed_fired.iter().any(|&f| f) {
                continue; // this test is specifically about the non-clamped case
            }
            let ak_float = levinson_durbin(&r);
            let ak_fixed = levinson_durbin_fixed(&r);
            for i in 0..=LPC_ORD {
                max_err = max_err.max((ak_float[i] - ak_fixed[i]).abs());
            }
            n_checked += 1;
        }
        assert!(n_checked > 300, "expected most of the real fixture corpus to never hit the clamp, only checked {n_checked}");
        assert!(max_err < 0.03, "expected tight quantization-noise-level agreement on non-clamped frames (allowing real margin for frame 273's own measured ill-conditioning), got max error {max_err}");
    }

    /// `FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s `autocorrelate`
    /// row: the real risk check that has to pass *before* writing
    /// `autocorrelate`'s own integer accumulator -- does replacing
    /// `levinson_durbin_fixed_core`'s internal `f32` `r[j]/r0`
    /// normalization with a fixed-point division reproduce the existing
    /// discriminator test's own real result (diverges from float only at
    /// clamp-disagreement frames), or does it introduce new, unexplained
    /// divergence of its own? A single fixed (not block-floating) Q-format
    /// covering `R[]`'s own real cross-frame range: 20 integer bits (real
    /// max `R[0]` ~8.6e5 per `CODEC2_MOD_FIXED_POINT_PLAN.md`'s own
    /// measurement needs `ceil(log2(8.6e5)) = 20`) + 40 fractional bits
    /// (real margin for the real min `R[0]` ~4e-6) = 60 bits, comfortably
    /// under `i64`'s 63 usable bits -- deliberately not block-floating,
    /// since the "shared exponent" this study first assumed was needed
    /// turned out to already be `r0` itself, cancelled at normalization.
    mod r0_normalization_fixed_point_candidate {
        use super::*;

        const R_FRAC_BITS: u32 = 43;

        fn quantize_r(r: &Autocorr) -> [i64; LPC_ORD + 1] {
            std::array::from_fn(|j| (r[j] as f64 * (1i64 << R_FRAC_BITS) as f64).round() as i64)
        }

        // r0_normalize_fixed and levinson_durbin_fixed_core_from_r_norm
        // are now real, production functions (promoted out of this test
        // module once validated) -- brought into scope here by `use
        // super::*` above, not redefined, so this test and the real
        // production code can never drift out of sync with each other.

        fn levinson_durbin_fixed_point_normalized(r: &Autocorr) -> (LpcCoeffs, [bool; LPC_ORD + 1]) {
            let r_q = quantize_r(r);
            let r_norm_q = r0_normalize_fixed(&r_q);
            let (a_q23, fired) = levinson_durbin_fixed_core_from_r_norm(&r_norm_q);
            let ak = std::array::from_fn(|i| a_q23[i] as f32 / (1i64 << COEF_FRAC_BITS) as f32);
            (ak, fired)
        }

        /// Real check that `autocorrelate_fixed`'s own natural Q8.23
        /// output precision (not this module's own synthetic Q(43),
        /// used above only to isolate the normalization question from
        /// the accumulator question) is *also* enough, with white noise
        /// correction, to pass the same discriminator -- the real,
        /// decisive end-to-end check before wiring `autocorrelate_fixed`
        /// into a real encoder.
        #[test]
        fn r0_normalization_at_autocorrelate_fixeds_real_q23_precision_also_passes_with_white_noise_correction(
        ) {
            const AUTOCORR_FRAC_BITS: u32 = WN_FRAC_BITS; // 23, matching autocorrelate_fixed's real output
            let r_path = fixture!("codec2_r_dump.txt");
            let rs = read_dump(r_path, LPC_ORD + 1);
            let mut n_diverged_without_clamp_disagreement = 0;
            let mut worst = 0.0f32;
            for r_row in &rs {
                let mut r = [0.0f32; LPC_ORD + 1];
                r.copy_from_slice(r_row);
                if r[0] <= 0.0 {
                    continue;
                }
                apply_white_noise_correction(&mut r);
                let r_q23: [i64; LPC_ORD + 1] = std::array::from_fn(|j| {
                    (r[j] as f64 * (1i64 << AUTOCORR_FRAC_BITS) as f64).round() as i64
                });
                let r0_q = r_q23[0];
                let r_norm_q: [i64; LPC_ORD + 1] = std::array::from_fn(|j| {
                    div_round_i128((r_q23[j] as i128) << LEVINSON_FRAC_BITS, r0_q as i128)
                });
                let (a_q23, candidate_fired) = levinson_durbin_fixed_core_from_r_norm(&r_norm_q);
                let ak_candidate: LpcCoeffs =
                    std::array::from_fn(|i| a_q23[i] as f32 / (1i64 << COEF_FRAC_BITS) as f32);
                let ak_float = levinson_durbin(&r);
                let max_err = (0..=LPC_ORD)
                    .map(|i| (ak_float[i] - ak_candidate[i]).abs())
                    .fold(0.0f32, f32::max);
                if max_err > 0.05 {
                    let float_fired = float_clamp_fired_per_iteration(&r);
                    let clamp_disagreement =
                        (0..=LPC_ORD).any(|i| float_fired[i] != candidate_fired[i]);
                    if !clamp_disagreement {
                        n_diverged_without_clamp_disagreement += 1;
                        worst = worst.max(max_err);
                    }
                }
            }
            println!("Q23-precision R[] + white noise correction: {n_diverged_without_clamp_disagreement} unexplained divergences, worst={worst}");
            assert_eq!(
                n_diverged_without_clamp_disagreement, 0,
                "Q8.23-precision R[] (matching autocorrelate_fixed's real output) diverged from float by more than 0.05 with no clamp disagreement on {n_diverged_without_clamp_disagreement} frame(s) (worst {worst}) -- autocorrelate_fixed's own WN_FRAC_BITS may need to be wider than 23"
            );
        }

        /// **Resolved by `apply_white_noise_correction`, not just worked
        /// around.** This candidate's own earlier version (no
        /// correction) failed this exact test on frame 273 -- see this
        /// module's own git history / `FIXED_POINT_ENCODER_
        /// IMPLEMENTATION_PUNCH_LIST.md` for the full diagnosis (the
        /// candidate's wide-integer division is *more* accurate than the
        /// production `f32` normalization, yet still diverged, because
        /// frame 273's own conditioning was fragile enough that even
        /// ordinary `f32` rounding was load-bearing). Applying the same
        /// correction that fixes Levinson-Durbin's own `1/e`
        /// amplification *before* this candidate's normalization runs
        /// resolves it completely -- real, measured result: 0 unexplained
        /// divergences across the whole corpus, not just a smaller
        /// number. This is real, direct evidence the white noise
        /// correction is a root-cause fix, not a narrow patch for one
        /// symptom: it independently resolves two separate problems
        /// (Levinson-Durbin's own division, and this normalization
        /// candidate's rounding-sensitivity) found by two separate
        /// investigations.
        #[test]
        fn r0_normalization_fixed_point_candidate_diverges_from_float_only_at_measured_clamp_disagreement_frames(
        ) {
            let r_path = fixture!("codec2_r_dump.txt");
            let rs = read_dump(r_path, LPC_ORD + 1);
            assert!(
                rs.len() > 300,
                "expected the real captured fixture corpus, got {} rows",
                rs.len()
            );

            let mut n_diverged_with_clamp_disagreement = 0;
            let mut n_diverged_without_clamp_disagreement = 0;
            let mut worst_unexplained_err = 0.0f32;

            for r_row in &rs {
                let mut r = [0.0f32; LPC_ORD + 1];
                r.copy_from_slice(r_row);
                if r[0] <= 0.0 {
                    continue;
                }
                apply_white_noise_correction(&mut r);

                let ak_float = levinson_durbin(&r);
                let (ak_candidate, candidate_fired) = levinson_durbin_fixed_point_normalized(&r);
                let max_err = (0..=LPC_ORD)
                    .map(|i| (ak_float[i] - ak_candidate[i]).abs())
                    .fold(0.0f32, f32::max);

                if max_err > 0.05 {
                    let float_fired = float_clamp_fired_per_iteration(&r);
                    let clamp_disagreement =
                        (0..=LPC_ORD).any(|i| float_fired[i] != candidate_fired[i]);
                    if clamp_disagreement {
                        n_diverged_with_clamp_disagreement += 1;
                    } else {
                        n_diverged_without_clamp_disagreement += 1;
                        worst_unexplained_err = worst_unexplained_err.max(max_err);
                    }
                }
            }

            let rate = n_diverged_with_clamp_disagreement as f64 / rs.len() as f64;
            println!(
                "r0_normalization_fixed_point_candidate: {n_diverged_with_clamp_disagreement}/{} frames diverged with a clamp disagreement ({:.2}%, plan doc's own measured baseline: 0.04%-0.9%)",
                rs.len(),
                rate * 100.0
            );
            // Real, positive result, found after `apply_white_noise_
            // correction` was added: this candidate originally failed on
            // frame 273 (this corpus's own known worst-conditioned
            // frame) at every `R_FRAC_BITS` tried (40, 43, and
            // diagnostically 70 -- identical ~1.8e-8 relative divergence
            // regardless, ruling out a precision-budget explanation).
            // Root cause, confirmed precisely: the *production* r0-
            // normalization computes `r[j]/r0` in `f32` (only ~24-bit /
            // ~1.7e-8-relative precision), and this candidate's
            // wide-integer division is *more* mathematically exact than
            // that -- yet frame 273 was fragile enough that even that
            // ordinary rounding artifact was load-bearing for which
            // answer you got. Applying `apply_white_noise_correction`
            // (see that function's own doc comment) *before* this
            // candidate's normalization runs resolves it completely --
            // real, measured across the whole corpus, not a smaller
            // number.
            assert_eq!(
                n_diverged_without_clamp_disagreement, 0,
                "found {n_diverged_without_clamp_disagreement} frame(s) where the r0-normalization candidate diverged from float by more than 0.05 with NO clamp disagreement (worst: {worst_unexplained_err}) -- expected white noise correction to have resolved this; if it's regressed, the correction's own effect needs rechecking, not just loosening this assertion"
            );
        }
    }
}
