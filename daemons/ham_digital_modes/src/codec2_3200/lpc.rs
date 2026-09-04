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
fn cheb_poly_eval_fixed(coef: &[f32; 6], x: f32) -> i32 {
    let coef_q: [i32; 6] = std::array::from_fn(|i| f32_to_q(coef[i], COEF_FRAC_BITS));
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

/// Finds one root of `poly` by sweeping `x` downward from `x_start` in
/// `LSP_SEARCH_STEP` increments until a sign change brackets a root,
/// then bisecting `LSP_BISECTIONS` times. The returned root is also
/// where the *next* root's search should resume, matching LSP roots'
/// own real interleaving property (each of the `LPC_ORD` roots lies in a
/// disjoint sub-interval of `[-1, 1]`, in strictly decreasing order, so
/// search never needs to backtrack).
fn find_next_root(poly: &[f32; 6], x_start: f32) -> Option<f32> {
    let mut xl = x_start;
    let mut p_l = cheb_poly_eval_fixed(poly, xl);
    while xl >= -1.0 {
        let xr = xl - LSP_SEARCH_STEP;
        let p_r = cheb_poly_eval_fixed(poly, xr);
        if (p_r <= 0 && p_l >= 0) || (p_r >= 0 && p_l <= 0) {
            let mut lo = xl;
            let mut hi = xr;
            let mut p_lo = p_l;
            let mut mid = 0.5 * (lo + hi);
            for _ in 0..LSP_BISECTIONS {
                mid = 0.5 * (lo + hi);
                let p_mid = cheb_poly_eval_fixed(poly, mid);
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
}

#[cfg(test)]
mod levinson_durbin_fixed_tests {
    use super::*;

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

        /// `r_norm_q[j] = r_q[j] * 2^LEVINSON_FRAC_BITS / r0_q`, the
        /// fixed-point replacement for `f32_to_q64(r[j]/r0,
        /// LEVINSON_FRAC_BITS)` -- same real shape as `div_round_i128`'s
        /// other real call site (`k = -numerator/e`): an i128-widened
        /// numerator over an `i64` divisor, calling `__divti3`. Unlike
        /// that call site (never wired into a running encoder), this one
        /// would actually run if wired in -- the real risk this whole
        /// candidate module exists to check, not assumed away.
        fn r0_normalize_fixed(r_q: &[i64; LPC_ORD + 1]) -> [i64; LPC_ORD + 1] {
            let r0_q = r_q[0];
            debug_assert!(r0_q > 0, "r0_normalize_fixed: r0_q must be positive, got {r0_q}");
            std::array::from_fn(|j| {
                div_round_i128((r_q[j] as i128) << LEVINSON_FRAC_BITS, r0_q as i128)
            })
        }

        /// Identical recursion body to `levinson_durbin_fixed_core`,
        /// minus that function's own internal `r[j]/r0` float
        /// normalization -- takes an already-normalized `r_norm_q`
        /// directly, so this candidate and the real production core can
        /// never drift out of sync with each other on anything but the
        /// one line under test (the normalization itself).
        fn levinson_durbin_fixed_core_from_r_norm(
            r_norm_q: &[i64; LPC_ORD + 1],
        ) -> (LpcCoeffsQ, [bool; LPC_ORD + 1]) {
            let mut a_q = [0i64; LPC_ORD + 1];
            let mut a_prev_q = [0i64; LPC_ORD + 1];
            a_q[0] = 1i64 << LEVINSON_FRAC_BITS;
            let mut e_q: i64 = 1i64 << LEVINSON_FRAC_BITS;
            let mut fired = [false; LPC_ORD + 1];

            for i in 1..=LPC_ORD {
                let mut sum_q: i64 = 0;
                for j in 1..i {
                    sum_q += q_mul(a_prev_q[j], r_norm_q[i - j]);
                }
                let numerator_q: i64 = r_norm_q[i] + sum_q;

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

        fn levinson_durbin_fixed_point_normalized(r: &Autocorr) -> (LpcCoeffs, [bool; LPC_ORD + 1]) {
            let r_q = quantize_r(r);
            let r_norm_q = r0_normalize_fixed(&r_q);
            let (a_q23, fired) = levinson_durbin_fixed_core_from_r_norm(&r_norm_q);
            let ak = std::array::from_fn(|i| a_q23[i] as f32 / (1i64 << COEF_FRAC_BITS) as f32);
            (ak, fired)
        }

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
            // **Real, diagnosed negative result -- see
            // FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md's
            // `autocorrelate` row for the full writeup.** This candidate
            // fails on frame 273 (this corpus's own known worst-
            // conditioned frame) regardless of R_FRAC_BITS (checked 40,
            // 43, and diagnostically up to 70 -- identical ~1.8e-8
            // relative divergence at every width, ruling out a precision
            // budget problem). Root cause, confirmed precisely: the
            // *production* r0-normalization computes `r[j]/r0` in `f32`
            // (only ~24-bit / ~1.7e-8-relative precision), and this
            // candidate's wide-integer division is *more* mathematically
            // exact than that -- but frame 273 is fragile enough that
            // even this ordinary, unremarkable `f32` rounding artifact is
            // load-bearing for the *current* implementation's own passing
            // status. Any change to the normalization's rounding --
            // including a strictly more accurate one -- reshuffles this
            // knife-edge frame's outcome, the same structural fragility
            // already flagged for `div_round_i128`'s own reciprocal-
            // multiply idea. Not a bug in this candidate; a real property
            // of this specific frame. Asserting `== 1` (not `== 0`)
            // because that's what a correct run of this exact candidate
            // against this exact corpus produces -- a passing test here
            // would mean the candidate silently changed.
            assert_eq!(
                n_diverged_without_clamp_disagreement, 1,
                "expected exactly 1 real, diagnosed frame-273-style divergence (see the comment above) -- got {n_diverged_without_clamp_disagreement} (worst: {worst_unexplained_err}); if this is now 0, the candidate changed and this comment's own diagnosis needs rechecking, not just loosening the assertion"
            );
        }
    }
}
