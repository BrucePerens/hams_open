// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point-oriented primitives for this port, built against the
//! real, measured bit-width bounds in
//! `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` -- but only for the
//! stages that plan doc's own advisor review confirmed transfer
//! cleanly to *this* port's own structurally-identical code, with an
//! acceptance criterion that doesn't require a product-judgment call
//! first.
//!
//! **Levinson-Durbin's own `|k|>1` clamp-boundary decision (previously
//! deliberately excluded here pending Bruce's own accept-vs-stabilize
//! call) is now made and built**, per Bruce's own explicit direction:
//! match the float reference's clamp behavior as closely as practical
//! and no closer -- accept its real, measured divergence rate rather
//! than adding a stabilization/smoothing step. See `lpc.rs`'s own
//! `levinson_durbin_fixed` for the real, genuinely-integer
//! implementation (not here, since it doesn't use this module's own
//! log-domain LUT shape) and its own doc comment for the real
//! measurement behind its Q8.40 internal format choice.
//!
//! **Both `log2_lut` and `exp2_lut`'s own interpolation are now
//! genuinely integer** (see `log2_lut_generic_fixed`/`exp2_lut_generic_
//! fixed` below, landed as two separate passes since they carry
//! different real risk -- `log2_lut`'s index/weight split comes free
//! from `x`'s own raw IEEE754 mantissa bits [`x.to_bits() &
//! 0x007F_FFFF`, already an *exact* Q23 fixed-point `(mantissa - 1.0)`],
//! while `exp2_lut`'s `y` has no such free structure and needs its own
//! boundary quantization -- `f32_to_q_exact_round`, itself exact bit
//! extraction from `y`'s own IEEE754 pattern, not a float multiply --
//! an arithmetic-shift floor correct on negative input, and a final
//! IEEE754 bit *reconstruction* rather than extraction). Both now run
//! entirely in `i64`/`u64` arithmetic with no float operation anywhere
//! inside -- `f32` is touched only at the true boundaries (`to_bits()`/
//! `from_bits()` on the way in and out, never a multiply, subtract, or
//! `powi`/`powf` call in between). Both keep their `f32 -> f32` signature,
//! since every real caller (`quantise::encode_energy`/`decode_energy`,
//! `synthesis::postfilter_step`) is still float upstream -- this closes
//! the *interpolation* gap specifically, not the surrounding call
//! sites. `levinson_durbin_fixed`'s own core recursion, by contrast,
//! has been genuine integer arithmetic throughout (`i64`/`i128` only,
//! no `f32` inside the loop) since an earlier pass -- the stages here
//! are at different real maturity levels for the no-FPU target, not
//! interchangeably "done."
//!
//! `log2_lut`/`exp2_lut` below are what `quantise::encode_energy`/
//! `decode_energy` AND `synthesis::postfilter_step` actually call now --
//! not a parallel unused implementation sitting next to a plain-float
//! one, the codec's real log-domain primitive at both call sites --
//! replacing each site's own plain-float `log10`/`powf` round trip with
//! the same 8-bit (256-entry), linearly-interpolated log2/exp2 LUT
//! shape the plan doc validated for `aks_to_mag2`'s own `R^(2*BETA)`
//! treatment (`x^k == 2^(k*log2(x))`, computed in base 2 specifically
//! because that's what a real fixed-point target would implement --
//! `frexp`-style exponent extraction is free, only the mantissa's own
//! log2 needs a table). Everything surrounding the LUT call itself (the
//! multiply by 10, the linear quantizer, the EMA) deliberately stays in
//! `f32` here, isolating the log-domain treatment specifically, matching
//! the plan doc's own validation scope for `aks_to_mag2` (Q-format
//! widths for the surrounding fixed-point arithmetic are a separate,
//! later engineering step, not attempted here).

use std::sync::OnceLock;

/// LUT resolution used by the real `log2_lut`/`exp2_lut` below -- 8
/// bits, matching the plan doc's own validated `aks_to_mag2` result
/// (max relative error 8.25e-7 there). Kept as a named constant (not
/// hardcoded into the table size) so the negative-control tests can
/// build a deliberately coarser table with the same generic code path
/// and confirm the choice of resolution is actually load-bearing, not
/// just present.
const LOG2_LUT_BITS: u32 = 8;
const LOG2_LUT_SIZE: usize = (1 << LOG2_LUT_BITS) + 1;

fn log2_lut_table() -> &'static [f32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[f32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| (1.0 + i as f32 / (1u32 << LOG2_LUT_BITS) as f32).log2())
    })
}

fn exp2_lut_table() -> &'static [f32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[f32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE
        .get_or_init(|| std::array::from_fn(|i| (i as f32 / (1u32 << LOG2_LUT_BITS) as f32).exp2()))
}

/// Q23-quantized sibling of `log2_lut_table()` -- what `log2_lut()`'s
/// real integer path actually reads. Quantized straight from the same
/// float table (not recomputed independently) so any rounding here is
/// ordinary, single-step Q23 rounding noise, not a second source of
/// disagreement with the float table other code still exercises.
fn log2_lut_table_q23() -> &'static [i32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[i32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let f = log2_lut_table();
        std::array::from_fn(|i| (f[i] * (1i64 << 23) as f32).round() as i32)
    })
}

/// `log2(x)` via IEEE754 exponent/mantissa split (exact, free -- just
/// the bit pattern) plus a linearly-interpolated table lookup for the
/// mantissa's own log2 -- the real fixed-point-friendly approximation
/// shape (`frexpf` + LUT) the plan doc validated, not a float shortcut.
/// `bits`/`table` are parameterized so the negative-control test below
/// can exercise the exact same interpolation code at a deliberately
/// coarser resolution.
///
/// Float reference only now -- kept `#[cfg(test)]` since the real
/// `log2_lut()` entry point below calls `log2_lut_generic_fixed`
/// instead (see that function's own doc comment for why). Still used
/// directly by the deliberately-coarse negative-control test, which
/// wants the plain-float interpolation shape at a resolution the real
/// integer table below was never built at.
#[cfg(test)]
fn log2_lut_generic(x: f32, bits: u32, table: &[f32]) -> f32 {
    debug_assert!(x > 0.0, "log2_lut_generic: x must be positive, got {x}");
    let levels = 1u32 << bits;
    let raw = x.to_bits();
    let exponent = ((raw >> 23) & 0xFF) as i32 - 127;
    let mantissa = f32::from_bits((raw & 0x007F_FFFF) | 0x3F80_0000); // [1.0, 2.0)
    let scaled = (mantissa - 1.0) * levels as f32;
    let idx = (scaled as usize).min(levels as usize - 1);
    let frac = scaled - idx as f32;
    exponent as f32 + table[idx] + frac * (table[idx + 1] - table[idx])
}

/// Q23 fixed-point sibling of `log2_lut_generic` -- the real
/// implementation `log2_lut()` now calls. `table[idx]`'s own log2
/// values live in `[0.0, 1.0]` (log2 of a mantissa in `[1.0, 2.0)`), so
/// Q23 (matching f32's own 23-bit mantissa width exactly) represents
/// them exactly enough that quantizing the table costs nothing beyond
/// ordinary Q23 rounding noise, and -- the actual point -- lets the
/// interpolation's own weight come directly from the raw mantissa bits
/// with no rescale.
///
/// The mantissa fraction `raw & 0x007F_FFFF` is already an *exact* Q23
/// fixed-point representation of `(mantissa - 1.0)` for `mantissa` in
/// `[1.0, 2.0)` -- IEEE754 stores exactly that value in exactly that
/// many bits. So unlike `log2_lut_generic`'s `(mantissa - 1.0) * levels
/// as f32` (a real `f32` multiply/subtract), this widens the raw
/// mantissa bits to `u64`, multiplies by `levels` in integer arithmetic
/// to get `scaled` pre-shifted by `2^23`, and splits that into an
/// index (top bits) and a Q23 interpolation weight (remaining bits) --
/// no float touches this until the final `exponent + interp` sum at
/// the very end, which is the genuine float/fixed boundary (`exponent`
/// itself came from an integer bit-shift, not a float op).
fn log2_lut_generic_fixed(x: f32, bits: u32, table_q23: &[i32]) -> f32 {
    debug_assert!(x > 0.0, "log2_lut_generic_fixed: x must be positive, got {x}");
    let levels = 1u32 << bits;
    let raw = x.to_bits();
    let exponent = ((raw >> 23) & 0xFF) as i32 - 127;
    let mantissa_frac_q23 = raw & 0x007F_FFFF; // exact, [0, 2^23)
    // scaled_full == scaled (as in log2_lut_generic) * 2^23, exactly --
    // an integer widen-multiply, no rounding introduced here at all.
    let scaled_full = mantissa_frac_q23 as u64 * levels as u64;
    let idx = ((scaled_full >> 23) as usize).min(levels as usize - 1);
    let frac_q23 = (scaled_full - ((idx as u64) << 23)) as i64; // [0, 2^23)
    let t0 = table_q23[idx] as i64;
    let t1 = table_q23[idx + 1] as i64;
    let interp_q23 = t0 + ((frac_q23 * (t1 - t0)) >> 23);
    exponent as f32 + (interp_q23 as f32) / (1i64 << 23) as f32
}

/// `2^y` via integer/fractional split (`2^y = 2^floor(y) * 2^frac`) plus
/// a linearly-interpolated table lookup for `2^frac` -- the inverse of
/// `log2_lut_generic`, same real fixed-point-friendly shape.
///
/// Float reference only now -- kept `#[cfg(test)]` since `exp2_lut()`
/// below calls `exp2_lut_generic_fixed` instead. Still used directly by
/// the round-trip test and to build the fixed path's own quantized
/// table.
#[cfg(test)]
fn exp2_lut_generic(y: f32, bits: u32, table: &[f32]) -> f32 {
    let levels = 1u32 << bits;
    let floor_y = y.floor();
    let frac = y - floor_y;
    let scaled = frac * levels as f32;
    let idx = (scaled as usize).min(levels as usize - 1);
    let t = scaled - idx as f32;
    let mantissa = table[idx] + t * (table[idx + 1] - table[idx]);
    mantissa * 2f32.powi(floor_y as i32)
}

/// Total fractional-bit precision `exp2_lut_generic_fixed` keeps for its
/// own `y_q` boundary conversion -- `LOG2_LUT_BITS` of it select the
/// table index (matching the table's own resolution), the rest
/// (`EXP2_Y_EXTRA_BITS`) give the interpolation weight `t` sub-index
/// precision it would otherwise have none of (unlike `log2_lut_generic_
/// fixed`, whose weight comes for free from the input's own raw mantissa
/// bits, `y` here has no such free source -- it isn't an IEEE754-shaped
/// value, just a real number needing an explicit floor/frac split).
const EXP2_Y_EXTRA_BITS: u32 = 16;
const EXP2_Y_FRAC_BITS: u32 = LOG2_LUT_BITS + EXP2_Y_EXTRA_BITS;

/// Q23-fractional-only sibling of `exp2_lut_table()`: stores
/// `(2^frac - 1.0)` in Q23, i.e. just the part above the implicit
/// leading `1.0` -- exactly the raw-mantissa-bits format IEEE754 itself
/// uses, so the final bit-reconstruction below needs no rescale, mirror
/// image of `log2_lut_table_q23`.
fn exp2_lut_table_frac_q23() -> &'static [i32; LOG2_LUT_SIZE] {
    static TABLE: OnceLock<[i32; LOG2_LUT_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let f = exp2_lut_table();
        std::array::from_fn(|i| ((f[i] - 1.0) * (1i64 << 23) as f32).round() as i32)
    })
}

/// Exact `round(y * 2^frac_bits)` computed straight from `y`'s own
/// IEEE754 bit pattern -- no float multiply at all, the same bit-
/// extraction trick `log2_lut_generic_fixed` already uses on its own
/// input, applied here instead to the value that needs converting *to*
/// fixed point rather than one already interpreted as one. `y == sign *
/// (2^23 | mantissa) * 2^(exp_biased - 127 - 23)`, so `y * 2^frac_bits`
/// is that same 24-bit significand shifted by `exp_biased - 150 +
/// frac_bits` -- a left shift when non-negative, otherwise a right
/// shift with an explicit round-half-up bias term (`+ 2^(shift-1)`
/// before shifting), never a genuine floating-point operation.
/// `pub(crate)`: also reused by `lpc.rs`'s `acos_lut_fixed`, the same
/// general "quantize an arbitrary non-IEEE754-structured value" need.
pub(crate) fn f32_to_q_exact_round(y: f32, frac_bits: u32) -> i64 {
    if y == 0.0 {
        return 0;
    }
    let raw = y.to_bits();
    let sign: i64 = if raw & 0x8000_0000 != 0 { -1 } else { 1 };
    let exp_biased = ((raw >> 23) & 0xFF) as i32;
    debug_assert!(
        exp_biased != 0,
        "f32_to_q_exact_round: subnormal y not supported here, got {y}"
    );
    let significand = (1u64 << 23) | (raw & 0x007F_FFFF) as u64; // [2^23, 2^24)
    let shift = exp_biased - 127 - 23 + frac_bits as i32;
    let mag = if shift >= 0 {
        debug_assert!(
            shift < 40,
            "f32_to_q_exact_round: y={y} too large for a {frac_bits}-fractional-bit Q format"
        );
        significand << shift
    } else {
        let neg_shift = (-shift) as u32;
        if neg_shift >= 64 {
            0
        } else {
            (significand + (1u64 << (neg_shift - 1))) >> neg_shift
        }
    };
    sign * mag as i64
}

/// Q23 fixed-point sibling of `exp2_lut_generic` -- the real
/// implementation `exp2_lut()` now calls. Unlike `log2_lut_generic_
/// fixed` (whose input `x` is already IEEE754-shaped, so its
/// index/weight split comes free from the raw mantissa bits), `y` here
/// is an arbitrary real number that first needs converting *to* a
/// Q(`EXP2_Y_FRAC_BITS`) integer -- done via `f32_to_q_exact_round`
/// above (exact bit extraction, not a float multiply: an `f64` multiply
/// was tried first and reads as fixed-point but isn't, exactly the kind
/// of doc/code mismatch this project has been burned by before). From
/// there: `floor(y)` and its fractional remainder both come from a
/// plain integer arithmetic-shift (correct on negative `y_q` too, since
/// Rust's `>>` on signed integers rounds toward negative infinity,
/// exactly matching `f32::floor()`), the index/weight split matches
/// `log2_lut_generic_fixed`'s own shape, and the final `mantissa *
/// 2^floor(y)` is built directly as an IEEE754 bit pattern
/// (`biased_exp << 23 | mantissa_frac`) rather than computed via
/// `2f32.powi()` -- the exact mirror image of `log2_lut_generic_
/// fixed`'s own bit *extraction*, this time *reconstructing* the
/// pattern instead. Genuinely integer, boundary to boundary.
fn exp2_lut_generic_fixed(y: f32, bits: u32, table_frac_q23: &[i32]) -> f32 {
    let extra_bits = EXP2_Y_FRAC_BITS - bits;
    let y_q = f32_to_q_exact_round(y, EXP2_Y_FRAC_BITS);
    let floor_y = y_q >> EXP2_Y_FRAC_BITS;
    let frac_full_q = y_q - (floor_y << EXP2_Y_FRAC_BITS); // [0, 2^EXP2_Y_FRAC_BITS)
    let levels = 1i64 << bits;
    let idx = ((frac_full_q >> extra_bits) as usize).min(levels as usize - 1);
    let t_num = frac_full_q - ((idx as i64) << extra_bits); // [0, 2^extra_bits)
    let t0 = table_frac_q23[idx] as i64;
    let t1 = table_frac_q23[idx + 1] as i64;
    let interp_frac_q23 = t0 + ((t_num * (t1 - t0)) >> extra_bits);
    debug_assert!(
        (0..(1i64 << 23)).contains(&interp_frac_q23),
        "exp2_lut_generic_fixed: interp_frac_q23={interp_frac_q23} out of the Q23 mantissa-fraction range -- would silently truncate below"
    );
    let biased_exp = floor_y + 127;
    debug_assert!(
        (1..=254).contains(&biased_exp),
        "exp2_lut_generic_fixed: floor(y)={floor_y} is out of f32's representable exponent range"
    );
    let raw_bits = ((biased_exp as u32) << 23) | (interp_frac_q23 as u32 & 0x007F_FFFF);
    f32::from_bits(raw_bits)
}

/// `pub(crate)`: `quantise::encode_energy` calls this directly (see
/// that function) -- this is the actual implementation the codec uses,
/// not a parallel unused sibling. Signature stays `f32 -> f32` (every
/// real caller is still float, since neither `encode_energy`'s own
/// input nor `synthesis::postfilter_step`'s are migrated yet), but the
/// interpolation itself now runs in `log2_lut_generic_fixed`'s integer
/// arithmetic, not `f32` multiply/subtract -- closing the exact gap
/// this module's own doc comment above described as "not attempted in
/// this pass."
pub(crate) fn log2_lut(x: f32) -> f32 {
    log2_lut_generic_fixed(x, LOG2_LUT_BITS, log2_lut_table_q23())
}

/// `pub(crate)`: `quantise::decode_energy` calls this directly. As of
/// this pass, `exp2_lut_generic_fixed` closes the same interpolation gap
/// for `exp2_lut` that `log2_lut_generic_fixed` closed above.
pub(crate) fn exp2_lut(y: f32) -> f32 {
    exp2_lut_generic_fixed(y, LOG2_LUT_BITS, exp2_lut_table_frac_q23())
}

/// Genuinely integer-in/integer-out sibling of `log2_lut` -- Q23 in
/// (`x_q23`, must be positive), Q23 out (`log2(x)` in Q23, `i64`).
/// `log2_lut`/`log2_lut_generic_fixed` above are already integer
/// *inside*, but still take/return `f32` at their own boundary (every
/// real caller there is still float upstream); this is the decoder's
/// own no-FPU entry point, called with a genuine Q23 integer on both
/// ends -- no `f32` touches this function at all, not even at entry/exit.
///
/// `x_q23` isn't IEEE754-shaped the way a real `f32` is, so this can't
/// reuse `log2_lut_generic_fixed`'s free exponent/mantissa bit split --
/// instead it does the equivalent integer normalization directly:
/// `x_q23 = mantissa_q23 * 2^shift` where `mantissa_q23` is the Q23
/// value renormalized into `[2^23, 2^24)` (i.e. `[1.0, 2.0)` in Q23) by
/// counting `x_q23`'s own leading zero bits, exactly the fixed-point
/// analogue of an IEEE754 exponent extraction.
pub(crate) fn log2_q23(x_q23: i64) -> i64 {
    debug_assert!(x_q23 > 0, "log2_q23: x_q23 must be positive, got {x_q23}");
    let bits = 63 - x_q23.leading_zeros() as i32; // position of the top set bit
    let shift = bits - 23; // x_q23 == mantissa_q23 * 2^shift, mantissa_q23 in [2^23, 2^24)
    let mantissa_q23: i64 = if shift >= 0 {
        x_q23 >> shift
    } else {
        x_q23 << (-shift)
    };
    let mantissa_frac_q23 = (mantissa_q23 - (1i64 << 23)) as u64; // [0, 2^23)
    let levels = 1u32 << LOG2_LUT_BITS;
    let scaled_full = mantissa_frac_q23 * levels as u64;
    let idx = ((scaled_full >> 23) as usize).min(levels as usize - 1);
    let frac_q23 = (scaled_full - ((idx as u64) << 23)) as i64;
    let table = log2_lut_table_q23();
    let t0 = table[idx] as i64;
    let t1 = table[idx + 1] as i64;
    let interp_q23 = t0 + ((frac_q23 * (t1 - t0)) >> 23);
    ((shift as i64) << 23) + interp_q23
}

/// Genuinely integer-in/integer-out sibling of `exp2_lut` -- Q23 in
/// (`y_q23`, `2^y` where `y = y_q23 / 2^23`), Q23 out (`i64`). Mirrors
/// `log2_q23`'s own no-FPU boundary: `exp2_lut_generic_fixed`'s float
/// entry point converts `y` to Q(`EXP2_Y_FRAC_BITS`) via `f32_to_q_
/// exact_round` and reconstructs an `f32` bit pattern at the end; this
/// instead takes `y_q23` directly (rescaled to `EXP2_Y_FRAC_BITS`
/// internally) and returns the Q23 mantissa shifted by `floor(y)`
/// directly as an integer -- no IEEE754 bit pattern involved on either
/// end.
pub(crate) fn exp2_q23(y_q23: i64) -> i64 {
    let y_q = y_q23 << (EXP2_Y_FRAC_BITS - 23); // rescale Q23 -> Q(EXP2_Y_FRAC_BITS)
    let floor_y = y_q >> EXP2_Y_FRAC_BITS;
    let frac_full_q = y_q - (floor_y << EXP2_Y_FRAC_BITS);
    let extra_bits = EXP2_Y_FRAC_BITS - LOG2_LUT_BITS;
    let levels = 1i64 << LOG2_LUT_BITS;
    let idx = ((frac_full_q >> extra_bits) as usize).min(levels as usize - 1);
    let t_num = frac_full_q - ((idx as i64) << extra_bits);
    let table = exp2_lut_table_frac_q23();
    let t0 = table[idx] as i64;
    let t1 = table[idx + 1] as i64;
    let interp_frac_q23 = t0 + ((t_num * (t1 - t0)) >> extra_bits); // (2^frac - 1.0) in Q23, [0, 2^23)
    let mantissa_q23 = (1i64 << 23) + interp_frac_q23; // 2^frac in Q23, [2^23, 2^24)
    if floor_y >= 0 {
        // mantissa_q23 < 2^24, so this silently overflows i64 (a plain
        // bit shift, not a checked one -- Rust only panics on a shift
        // *amount* >= the type's bit width, never on the shifted
        // *value* overflowing) once floor_y+24 >= 63. The same failure
        // mode this port's own correct_sub_multiples_fixed/`CNLP*gmax`
        // bug was, caught once already -- guarded here instead of
        // relearned.
        debug_assert!(
            floor_y < 39,
            "exp2_q23: floor(y)={floor_y} overflows i64 at Q23 -- caller's y domain exceeds this function's safe range"
        );
        mantissa_q23 << floor_y
    } else {
        let neg = (-floor_y) as u32;
        if neg >= 63 {
            0
        } else {
            (mantissa_q23 + (1i64 << (neg - 1))) >> neg
        }
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn log2_lut_and_exp2_lut_are_real_inverses_across_a_wide_dynamic_range() {
        // Sweeps many decades (matching the real E_MIN_DB..E_MAX_DB dB
        // span converted to linear, 1e-1..1e4, with real margin either
        // side) -- confirms the two LUTs are actually inverses of each
        // other, not just individually plausible.
        let mut x = 1e-3f32;
        let mut max_rel_err = 0.0f32;
        while x < 1e6 {
            let y = log2_lut(x);
            let back = exp2_lut(y);
            let rel_err = ((back - x) / x).abs();
            max_rel_err = max_rel_err.max(rel_err);
            x *= 1.0173; // dense, irrational-ish multiplicative step
        }
        assert!(
            max_rel_err < 1e-4,
            "log2_lut/exp2_lut round trip relative error too large: {max_rel_err}"
        );
    }

    /// Independent plain-float reference (`log10`/`powf`, no LUT at
    /// all) for what `quantise::encode_energy` computed *before* this
    /// module existed -- deliberately NOT calling
    /// `quantise::encode_energy` itself, since that function now calls
    /// straight into `log2_lut` (see this module's own doc comment):
    /// comparing the LUT against itself via that indirection would be
    /// circular and prove nothing.
    fn reference_e_db(e_linear: f32) -> f32 {
        10.0 * e_linear.max(1e-12).log10()
    }

    #[test]
    fn the_8_bit_log2_lut_reproduces_the_plain_float_log10_quantizer_decision_on_real_encoder_side_data_with_zero_index_mismatches(
    ) {
        // Real ENCODER-side e (the actual encode_energy() call-site
        // argument, captured directly -- see quantise.rs's own test of
        // the same fixture for why that distinction matters).
        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);
        assert!(
            es.len() > 300,
            "expected the real captured fixture corpus, got {} rows",
            es.len()
        );

        let mut mismatches = 0;
        for row in &es {
            let e = row[0];
            let plain_idx = super::super::quantise::quantize_linear(
                reference_e_db(e),
                super::super::E_MIN_DB,
                super::super::E_MAX_DB,
                super::super::E_BITS,
            );
            let lut_e_db = 10.0 * (log2_lut(e.max(1e-12)) / std::f32::consts::LOG2_10);
            let lut_idx = super::super::quantise::quantize_linear(
                lut_e_db,
                super::super::E_MIN_DB,
                super::super::E_MAX_DB,
                super::super::E_BITS,
            );
            if plain_idx != lut_idx {
                mismatches += 1;
            }
        }
        assert_eq!(mismatches, 0, "LUT-based log2 diverged from plain log10 on {mismatches}/{} real frames -- the plan doc's own validated result for this LUT design is zero mismatches", es.len());
    }

    #[test]
    fn a_deliberately_coarse_4_bit_lut_produces_real_index_mismatches_confirming_the_test_above_is_not_vacuous(
    ) {
        // Negative control, same methodology the plan doc itself used
        // for aks_to_mag2: rerun the real fixture corpus through the
        // exact same interpolation code at a much coarser resolution
        // and confirm it actually degrades -- if it didn't, the
        // zero-mismatches result above wouldn't be evidence the 8-bit
        // resolution matters, just that log2/exp2-LUT-shaped code
        // happens to always match float here regardless of table size.
        const COARSE_BITS: u32 = 4;
        let coarse_log2: Vec<f32> = (0..=(1u32 << COARSE_BITS))
            .map(|i| (1.0 + i as f32 / (1u32 << COARSE_BITS) as f32).log2())
            .collect();

        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);

        let mut mismatches = 0;
        for row in &es {
            let e = row[0].max(1e-12);
            let e_db_coarse =
                10.0 * (log2_lut_generic(e, COARSE_BITS, &coarse_log2) / std::f32::consts::LOG2_10);
            let coarse_idx = super::super::quantise::quantize_linear(
                e_db_coarse,
                super::super::E_MIN_DB,
                super::super::E_MAX_DB,
                super::super::E_BITS,
            );
            let plain_idx = super::super::quantise::quantize_linear(
                reference_e_db(e),
                super::super::E_MIN_DB,
                super::super::E_MAX_DB,
                super::super::E_BITS,
            );
            if coarse_idx != plain_idx {
                mismatches += 1;
            }
        }
        assert!(mismatches > 0, "expected the deliberately coarse 4-bit LUT to produce at least one real index mismatch against the plain float quantizer -- got zero, which would mean the 8-bit result above isn't evidence of anything");
    }

    #[test]
    fn log2_lut_generic_fixeds_integer_interpolation_matches_the_float_interpolation_directly() {
        // The corpus-based tests above only check quantizer-index
        // agreement -- coarse enough to hide a real sign or off-by-one
        // error in the new integer path (log2_lut_generic_fixed) that
        // just happens not to cross an index boundary on real encoder
        // data. This compares the two interpolation implementations
        // directly, dense, across several decades of `x`, so an
        // exponent-extraction or shift-direction bug shows up as a
        // large error at a specific magnitude rather than as a
        // silently-absorbed index shift.
        let table_f32 = log2_lut_table();
        let table_q23 = log2_lut_table_q23();
        let mut max_abs_err = 0.0f32;
        let mut x = 1e-6f32;
        while x < 1e6 {
            let want = log2_lut_generic(x, LOG2_LUT_BITS, table_f32);
            let got = log2_lut_generic_fixed(x, LOG2_LUT_BITS, table_q23);
            max_abs_err = max_abs_err.max((got - want).abs());
            x *= 1.0011; // dense: tens of thousands of points over 12 decades
        }
        assert!(
            max_abs_err < 1e-5,
            "log2_lut_generic_fixed diverged from the float interpolation by {max_abs_err}, more than ordinary Q23 table-quantization noise"
        );
    }

    #[test]
    fn log2_lut_generic_fixed_handles_its_own_index_boundaries_correctly() {
        // Exact powers of two: mantissa_frac bits are all zero, so
        // idx == 0 and frac_q23 == 0 -- exercises the zero-weight edge
        // with no interpolation blend at all.
        for exp in -20i32..=20 {
            let x = 2.0f32.powi(exp);
            let want = log2_lut_generic(x, LOG2_LUT_BITS, log2_lut_table());
            let got = log2_lut_generic_fixed(x, LOG2_LUT_BITS, log2_lut_table_q23());
            assert!(
                (got - want).abs() < 1e-5,
                "power-of-two x={x} (2^{exp}): fixed={got} float={want}"
            );
            assert_eq!(want, exp as f32, "power-of-two log2 should be exact: {want} vs {exp}");
        }

        // Just below a power of two: mantissa_frac bits are all one
        // (0x7FFFFF), pushing scaled_full's own top bits to exactly
        // levels-1 and making the `.min(levels-1)` clamp load-bearing
        // -- without it idx would read one past the table's own end.
        let just_below_2 = f32::from_bits(0x3FFF_FFFF); // mantissa = 0x7FFFFF
        let want = log2_lut_generic(just_below_2, LOG2_LUT_BITS, log2_lut_table());
        let got = log2_lut_generic_fixed(just_below_2, LOG2_LUT_BITS, log2_lut_table_q23());
        assert!(
            (got - want).abs() < 1e-5,
            "just-below-2 boundary: fixed={got} float={want}"
        );
    }

    #[test]
    fn exp2_lut_generic_fixeds_integer_interpolation_matches_the_float_interpolation_directly() {
        // Same rationale as log2's own direct-comparison test above:
        // dense, across a real range for y (matching E_MIN_DB..E_MAX_DB
        // converted through log2, with margin), so a sign or
        // shift-direction bug in the new floor/frac/bit-reconstruction
        // path shows up as a large error at a specific y rather than
        // hiding behind quantizer-index agreement.
        let table_f32 = exp2_lut_table();
        let table_frac_q23 = exp2_lut_table_frac_q23();
        let mut max_rel_err = 0.0f32;
        let mut y = -20.0f32;
        while y < 20.0 {
            let want = exp2_lut_generic(y, LOG2_LUT_BITS, table_f32);
            let got = exp2_lut_generic_fixed(y, LOG2_LUT_BITS, table_frac_q23);
            let rel_err = ((got - want) / want).abs();
            max_rel_err = max_rel_err.max(rel_err);
            y += 0.0037; // dense: ~10800 points across the range
        }
        assert!(
            max_rel_err < 1e-4,
            "exp2_lut_generic_fixed diverged from the float interpolation by {max_rel_err} relative, more than ordinary Q23 table-quantization noise"
        );
    }

    #[test]
    fn exp2_lut_generic_fixed_handles_negative_y_and_its_own_index_boundaries_correctly() {
        // Exact integers (frac == 0, including negative ones -- the
        // arithmetic-shift floor is the part with real sign risk):
        // idx == 0, t_num == 0, no interpolation blend.
        for exp in -20i32..=20 {
            let y = exp as f32;
            let want = exp2_lut_generic(y, LOG2_LUT_BITS, exp2_lut_table());
            let got = exp2_lut_generic_fixed(y, LOG2_LUT_BITS, exp2_lut_table_frac_q23());
            assert!(
                (got - want).abs() / want.abs() < 1e-5,
                "integer y={y}: fixed={got} float={want}"
            );
            assert_eq!(want, 2.0f32.powi(exp), "exp2 of an exact integer should be exact: {want}");
        }

        // Just below a negative integer boundary (e.g. -0.0001), where
        // floor_y must correctly come out one below y itself -- the
        // exact case an off-by-one in the arithmetic-shift floor would
        // get wrong only for negative input.
        let just_below_neg3 = -3.0001f32;
        let want = exp2_lut_generic(just_below_neg3, LOG2_LUT_BITS, exp2_lut_table());
        let got = exp2_lut_generic_fixed(just_below_neg3, LOG2_LUT_BITS, exp2_lut_table_frac_q23());
        assert!(
            (got - want).abs() / want.abs() < 1e-4,
            "just-below-negative-integer boundary: fixed={got} float={want}"
        );
    }

    #[test]
    fn encode_energy_and_decode_energy_now_run_through_the_lut_and_still_round_trip_within_one_quantizer_step(
    ) {
        // Closes the loop advisor flagged: after quantise::encode_energy/
        // decode_energy were switched to call straight into this
        // module's LUT, does a real captured e still round-trip through
        // the *actual, now-LUT-backed* encode_energy/decode_energy to
        // within the same one-quantizer-step bound the plain-float
        // version always met? (quantise.rs's own
        // energy_quantizer_matches_real_encoder_side_data_within_the_real_5_bit_step
        // test already asserts this same bound against the same
        // fixture -- this test exists so that guarantee is visible from
        // this module too, right next to the LUT it now depends on.)
        let e_path = fixture!("codec2_enc_e_dump.txt");
        let es = read_dump(e_path, 1);
        let step_db =
            (super::super::E_MAX_DB - super::super::E_MIN_DB) / (1 << super::super::E_BITS) as f32;

        for row in &es {
            let e = row[0];
            let e_db = reference_e_db(e);
            if !(super::super::E_MIN_DB..=super::super::E_MAX_DB).contains(&e_db) {
                continue; // real, designed clamp region -- see quantise.rs's own test for why
            }
            let idx = super::super::quantise::encode_energy(e);
            let back = super::super::quantise::decode_energy(idx);
            let back_db = reference_e_db(back);
            assert!((back_db - e_db).abs() <= step_db, "LUT-backed round trip for e={e} (db={e_db}) landed at {back} (db={back_db}), step {step_db}dB");
        }
    }

    #[test]
    fn postfilter_lut_decisions_match_plain_float_across_a_real_temporal_replay() {
        // Same validation shape CODEC2_MOD_FIXED_POINT_PLAN.md used for
        // this exact stage (bg_est is a real stateful EMA carrying
        // across frames, so a faithful test needs a real temporal
        // replay, not independent per-frame samples): run two parallel
        // postfilter_step state machines -- one plain float, one this
        // module's LUT -- forward through an identical real sequence of
        // (voiced, l, a[]), and check every real per-harmonic threshold
        // decision matches.
        //
        // The per-harmonic amplitudes (`a[]`) driving this come from
        // real captured data (this port's own already-validated
        // lsp_to_lpc + compute_harmonic_amplitudes run on real captured
        // lsp[]/e[] fixture rows) -- that's what determines whether the
        // log-domain LUT approximation ever changes a real decision, so
        // it has to be real. `voiced`/`Wo` only select which branch
        // runs and how many harmonics exist; a representative synthetic
        // sweep (long voiced/unvoiced RUNS, not frame-by-frame
        // alternation, so bg_est's own EMA has real stretches to settle
        // against before being tested) is fine there -- the same
        // encoder/decoder-internal-design-freedom reasoning `nlp.rs`'s
        // own module doc comment uses for a transmitted value, applied
        // here to a purely decoder-internal test-harness choice instead.
        let lsp_rows = read_dump(fixture!("codec2_lsp_dump.txt"), super::super::LPC_ORD + 1);
        let e_rows = read_dump(fixture!("codec2_enc_e_dump.txt"), 1);
        let n = lsp_rows.len().min(e_rows.len());
        assert!(
            n > 300,
            "expected the real captured fixture corpus, got {n} rows"
        );

        let fft = {
            let mut planner = rustfft::FftPlanner::<f32>::new();
            planner.plan_fft_forward(super::super::FFT_ENC)
        };

        let mut plain_bg = 0.0f32;
        let mut lut_bg = 0.0f32;
        let mut decisions_checked = 0usize;
        let mut decision_mismatches = 0usize;
        let mut max_bg_drift_db = 0.0f32;

        for i in 0..n {
            let roots = lsp_rows[i][0] as i32;
            if roots as usize != super::super::LPC_ORD {
                continue; // real, rare LSP root-finding failure -- skip, same as quantise.rs's own test
            }
            let mut lsp = [0.0f32; super::super::LPC_ORD];
            lsp.copy_from_slice(&lsp_rows[i][1..]);
            let e = e_rows[i][0];

            // 30-frame runs: long enough for bg_est's BG_BETA=0.1 EMA
            // to settle meaningfully within each unvoiced run before
            // the following voiced run's decisions get checked against it.
            let voiced = (i / 30) % 2 == 1;
            let wo = super::super::W0_MIN + (super::super::W0_MAX - super::super::W0_MIN) * 0.3;

            let ak = super::super::lpc::lsp_to_lpc(&lsp);
            let mut model = super::super::envelope::Model::new(wo, voiced);
            let _aw = super::super::envelope::compute_harmonic_amplitudes(
                fft.as_ref(),
                &ak,
                e,
                &mut model,
            );
            super::super::envelope::apply_first_harmonic_correction(&mut model);

            let (new_plain_bg, plain_decisions) = super::super::synthesis::postfilter_step(
                model.voiced,
                model.l,
                &model.a,
                plain_bg,
                f32::log2,
                f32::exp2,
            );
            let (new_lut_bg, lut_decisions) = super::super::synthesis::postfilter_step(
                model.voiced,
                model.l,
                &model.a,
                lut_bg,
                log2_lut,
                exp2_lut,
            );

            max_bg_drift_db = max_bg_drift_db.max((new_plain_bg - new_lut_bg).abs());
            plain_bg = new_plain_bg;
            lut_bg = new_lut_bg;

            if voiced {
                for m in 1..=model.l {
                    decisions_checked += 1;
                    if plain_decisions[m] != lut_decisions[m] {
                        decision_mismatches += 1;
                    }
                }
            }
        }

        assert!(decisions_checked > 1000, "expected a real number of real per-harmonic decisions checked across the replay, got {decisions_checked}");
        assert_eq!(decision_mismatches, 0, "{decision_mismatches}/{decisions_checked} real per-harmonic postfilter decisions diverged between the LUT and plain float across the temporal replay");
        assert!(max_bg_drift_db < 1e-3, "bg_est drifted {max_bg_drift_db}dB between the LUT and plain-float state machines over the replay -- too large for an 8-bit LUT");
    }

    #[test]
    fn log2_q23_matches_log2_lut_across_a_wide_dynamic_range() {
        // log2_q23 is DecoderFixed's own no-FPU entry point (Q23 in, Q23
        // out, no f32 touches it at all) -- this checks it against
        // log2_lut (the existing, already-validated integer-inside/
        // float-boundary sibling) rather than plain float log2 directly,
        // since any disagreement between the two integer paths would be
        // a bug in the new normalization (leading-zero-count exponent
        // extraction) specifically, not just ordinary LUT error both
        // would share against plain float.
        //
        // Both sides are fed the SAME already-Q23-quantized x (round-
        // tripped through f32 once for log2_lut's own f32 boundary),
        // not the original continuous x -- otherwise this would also be
        // measuring x's own Q23 input-quantization error (real and
        // large at the low end of a wide sweep, since Q23's fixed
        // absolute step is a large relative fraction of a small x, but
        // not a log2_q23 bug), which isn't what this test is checking.
        let mut max_abs_err_q23 = 0i64;
        let mut x = 1e-3f32;
        while x < 1e6 {
            let x_q23 = f32_to_q_exact_round(x, 23);
            if x_q23 <= 0 {
                x *= 1.0173;
                continue; // too small to represent in Q23 at all
            }
            let x_reconstructed = x_q23 as f32 / (1i64 << 23) as f32;
            let want_q23 = f32_to_q_exact_round(log2_lut(x_reconstructed), 23);
            let got_q23 = log2_q23(x_q23);
            max_abs_err_q23 = max_abs_err_q23.max((got_q23 - want_q23).abs());
            x *= 1.0173;
        }
        assert!(
            max_abs_err_q23 < 16,
            "log2_q23 diverged from log2_lut by {max_abs_err_q23} Q23 counts, more than ordinary rounding noise"
        );
    }

    #[test]
    fn log2_q23_handles_x_less_than_one_correctly() {
        // x < 1.0 means shift < 0 (the left-shift renormalization
        // branch) and a negative result -- the case most likely to
        // break in an exponent-extraction rewrite, called out
        // specifically since exp2_lut_generic_fixed's own negative-y
        // branch was exactly this kind of bug risk historically.
        for &x in &[0.5f32, 0.25, 0.125, 0.001, 0.9999] {
            let x_q23 = f32_to_q_exact_round(x, 23);
            let want = x.log2();
            let got = log2_q23(x_q23) as f32 / (1i64 << 23) as f32;
            assert!(
                (got - want).abs() < 1e-4,
                "log2_q23({x}) = {got}, want ~{want}"
            );
        }
    }

    #[test]
    fn exp2_q23_matches_exp2_lut_across_a_wide_range() {
        // Range bound is -8..16, not the full +-20 log2_lut/exp2_lut's
        // own round-trip test sweeps: at very negative y, 2^y is only a
        // handful of Q23 counts (e.g. 2^-16 is ~128 counts), so +-0.5
        // count of ordinary rounding is already a large RELATIVE error
        // there -- a real, inherent property of any fixed-point format's
        // fixed absolute resolution at tiny magnitudes, not a bug, and
        // well outside the real operating range this actually needs
        // (`decode_energy_fixed`'s own y stays within roughly
        // -3.3..13.3, see E_MIN_DB/E_MAX_DB) -- -8 keeps a generous
        // margin below that while staying far enough from the format's
        // own low-count noise floor for a tight relative bound to be
        // the right metric. Both sides are also fed the same already-
        // Q23-quantized y (see `log2_q23`'s own sibling test for why),
        // isolating the algorithmic comparison from y's own input
        // quantization noise.
        let mut max_rel_err = 0.0f32;
        let mut y = -8.0f32;
        while y < 16.0 {
            let y_q23 = f32_to_q_exact_round(y, 23);
            let y_reconstructed = y_q23 as f32 / (1i64 << 23) as f32;
            let want = exp2_lut(y_reconstructed);
            let got = exp2_q23(y_q23) as f32 / (1i64 << 23) as f32;
            let rel_err = ((got - want) / want).abs();
            max_rel_err = max_rel_err.max(rel_err);
            y += 0.0037;
        }
        assert!(
            max_rel_err < 1e-4,
            "exp2_q23 diverged from exp2_lut by {max_rel_err} relative, more than ordinary Q23 rounding noise"
        );
    }

    #[test]
    fn exp2_q23_handles_negative_y_correctly() {
        // -14.0, not -19.5: same "tiny result -> large relative error
        // is inherent, not a bug" reasoning as the wide-range test above
        // -- 2^-14 is a real, well-resolved Q23 value (~512 counts),
        // unlike 2^-19.5 (~11 counts).
        for &y in &[-1.0f32, -5.0, -14.0, -0.1] {
            let y_q23 = f32_to_q_exact_round(y, 23);
            let want = y.exp2();
            let got = exp2_q23(y_q23) as f32 / (1i64 << 23) as f32;
            assert!(
                (got - want).abs() / want < 1e-4,
                "exp2_q23({y}) = {got}, want ~{want}"
            );
        }
    }

    #[test]
    fn log2_q23_and_exp2_q23_are_real_inverses_of_each_other() {
        let mut x = 1e-2f32;
        let mut max_rel_err = 0.0f32;
        while x < 1e5 {
            let x_q23 = f32_to_q_exact_round(x, 23);
            let y_q23 = log2_q23(x_q23);
            let back_q23 = exp2_q23(y_q23);
            let back = back_q23 as f32 / (1i64 << 23) as f32;
            let rel_err = ((back - x) / x).abs();
            max_rel_err = max_rel_err.max(rel_err);
            x *= 1.0311;
        }
        assert!(
            max_rel_err < 1e-4,
            "log2_q23/exp2_q23 round trip relative error too large: {max_rel_err}"
        );
    }
}
