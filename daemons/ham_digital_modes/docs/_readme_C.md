## Codec2 3200 LPC analysis (`src/codec2_3200/lpc.rs`)

The 3200 bit/s encoder turns each windowed speech frame into ten line spectral frequencies (LSPs) in four steps:
autocorrelation, Levinson-Durbin to get linear prediction coefficients (LPCs), bandwidth expansion, and LPC-to-LSP
conversion. The decoder goes the other way with `lsp_to_lpc`. The integer path works in Q8.23 (`COEF_FRAC_BITS` = 23)
everywhere except inside the Levinson recursion, which uses Q8.40.

### Autocorrelation and white noise correction

`[@ANCHOR: autocorrelate_fixed]` computes the eleven autocorrelation lags `R[0..=10]` of the windowed frame
`wn_q` (Q8.23, `i32`). For each lag `j` it sums `wn[i] * wn[i+j]` over the `len - j` overlapping samples; each product is
formed in `i64` and shifted right by 23 *before* it is added (an arithmetic shift, so it floors), which keeps `R` in Q8.23
and the running sum far inside `i64`. A frame shorter than `LPC_ORD` samples would underflow the loop bound, so callers
always pass the full 320-sample frame.

`[@ANCHOR: apply_white_noise_correction_fixed]` implements the standard "white noise correction" (Rabiner and Schafer,
1978): it replaces `R[0]` by `R[0] + R[0]/1000` (a 0.1 % noise floor, about 30 dB below the signal) and leaves
`R[1..]` alone. The division is exact integer division by the literal 1000, rounded to nearest, half away from zero. It
bounds how small the Levinson prediction error can become on very well predicted frames, which is what makes the
recursion's `1/e` division fragile. An all-zero `R` stays all-zero. The caller must apply it before the integer Levinson
entry points.

### Levinson-Durbin recursion

`[@ANCHOR: levinson_durbin_fixed]` is the historical `f32`-in entry point: it runs `levinson_durbin_fixed_core` on an
`f32` autocorrelation and converts the Q8.23 result back to `f32` coefficients (`a[0]` is 1.0). It debug-asserts
`R[0] > 0`. The production encoder uses the integer-input entry points in the same file instead; this one is kept as the
bridge to the floating reference and is what the divergence tests compare against `levinson_durbin`.

`[@ANCHOR: levinson_durbin_fixed_core]` normalizes the `f32` autocorrelation by `R[0]` (`r[j]/r[0]`, quantized to Q8.40
with `f32_to_q64`), then calls the shared recursion. It returns the Q8.23 coefficients and, per iteration, whether the
`|k| > 1` safety clamp fired; the tests use that flag to tell expected clamp-boundary divergence from real arithmetic bugs.

`[@ANCHOR: levinson_durbin_fixed_core_from_r_norm]` is the recursion proper, on an already normalized `r_norm_q`
(Q8.40, `r_norm[0]` taken as exactly 1.0 and never read). For `i = 1..=10` it forms the numerator
`r[i] + sum(a_prev[j] * r[i-j])`, the reflection coefficient `k = -numerator / e` (rounded integer division), snaps `k`
to zero when `|k| > 1.0` (the same clamp as the float reference), updates `a[j] = a_prev[j] + k * a_prev[i-j]` and
`e *= (1 - k^2)`. If `e` has reached exactly zero (a previous `k` of exactly +/-1) or the numerator is zero, `k` is set to
zero instead of dividing, so the function never traps on a division by zero. The Q8.40 result is rounded to Q8.23 at the end.

`[@ANCHOR: r0_normalize_fixed]` turns an integer autocorrelation `r_q` (any Q format) into `r_norm_q[j] = R[j] * 2^40 /
R[0]` with a rounded 128-bit division. If `R[0] <= 0` (a silent frame) it returns `[1.0, 0, 0, ...]` instead of dividing,
which makes the recursion return the identity filter `a = [1, 0, ..., 0]`. The caller must have applied white noise
correction first; the validation of this path assumes it.

`[@ANCHOR: rshift_round]` is `x >> n` rounded to nearest (add half, then shift). Plain `>>` floors and its bias, repeated
over ten iterations, produced a measurable coefficient error on the worst corpus frame. `n = 0` returns `x`. The
Levinson core uses it for the final Q8.40 to Q8.23 conversion.

`[@ANCHOR: q_mul]` multiplies two Q8.40 numbers: the 128-bit product plus half a unit, shifted right by 40, narrowed to
`i64` (debug-asserted not to overflow). `div_round_i128` (described in the codec2_3200 story) is its division counterpart.

### Q-format conversion helpers

`[@ANCHOR: dequantize_coef_q23]` converts Q8.23 coefficients to `f32` by dividing by 2^23, for callers that need the
float boundary without access to the crate-private `COEF_FRAC_BITS`.

`[@ANCHOR: f32_to_q]` converts an `f32` to a signed fixed-point `i32` with the given number of fractional bits: multiply
by 2^bits in `f64`, round half away from zero, cast (the cast saturates, so formats wider than 31 bits cannot be held).
`[@ANCHOR: f32_to_q64]` is the same conversion with an `i64` result, needed for the Q8.40 format of the Levinson recursion;
NaN converts to 0, which the silent-frame path relies on.

### LPC to LSP conversion

The ten LSPs are the roots, in `x = cos(omega)`, of the symmetric and antisymmetric polynomials `P` and `Q` built from
the coefficients (Sugamura and Itakura 1981). They interleave, so the search takes the roots alternately from `P` and `Q`,
each time starting just below the previous root.

`[@ANCHOR: build_p_q_fixed]` builds the six Chebyshev-basis coefficients of `P` and `Q` from Q8.23 `a_q23`: with
`p[0] = q[0] = 1`, `p[i] = a[i] + a[11-i] - p[i-1]` and `q[i] = a[i] - a[11-i] + q[i-1]` for `i = 1..=5`, then the first
five entries of each doubled. It is pure addition, so no precision is lost; results are narrowed to `i32` (the
coefficients stay under 128 in magnitude, which is why Q8.23 fits).

`[@ANCHOR: cheb_poly_eval_fixed_core]` evaluates `sum coef[5-i] * T_i(x)` for a real `x` given as `f32` against Q8.23
coefficients, by quantizing `x` to Q2.29 and running the Chebyshev three-term recurrence in the integer helper. Only the
sign of the result is used by the root search. It exists so the coefficients are quantized once per search rather than per
candidate `x`.

`[@ANCHOR: find_next_root_from_q23]` walks `x` downward from `x_start` in steps of 0.01 until the polynomial changes
sign between two neighbours, then bisects that bracket six times and returns the midpoint of the last bisection (about
0.00016 accurate). It returns `None` if no sign change is found before `x < -1`. It is `pub(crate)` because the float
reference's own `find_next_root` calls it.

`[@ANCHOR: lpc_to_lsp_from_integer_ak]` converts Q8.23 coefficients to ten LSP frequencies in radians (`f32`, ascending
from the first root near `x = 1`). It builds `P`/`Q`, finds each root with `find_next_root_from_q23` and converts it with
`acos_lut_fixed`; it returns `None` if any of the ten roots is not found. The all-integer form of the same step is
`lpc_to_lsp_q23_from_integer_ak`.

`[@ANCHOR: acos_lut_table_q23]` returns the 4097-entry table of `acos(i/4096)` in Q23 (generated into
`tables.rs`, entries within one unit of the exact value, with `acos(0) = pi/2` and `acos(1) = 0`), the input of the linear
interpolation in `acos_lut_q23`. `[@ANCHOR: pi_q23]` is pi in Q23 (26353590, the `f32` value of pi quantized); it is the
reflection constant for negative arguments in `acos_lut_q23` and the upper limit of `cos_q23`'s angle domain.

`[@ANCHOR: acos_lut_fixed]` is the `f32` wrapper of the integer `acos`: it quantizes `|x|` to Q23 (clamped to 1.0), restores
the sign, calls `acos_lut_q23` and converts the Q23 radians back to `f32`. `x` must lie in `[-1, 1]` (debug-asserted).
Worst-case error over the whole domain is about 7 Hz of an LSP at 8 kHz sampling, below the 25 Hz quantizer step.

### LSP to LPC conversion (decoder side)

`[@ANCHOR: lsp_to_lpc]` rebuilds `f32` LPC coefficients from ten LSP angles: `P(z)` is the product of
`1 - 2cos(w_i) z^-1 + z^-2` over the even-indexed LSPs times `(1 + z^-1)`, `Q(z)` the same over the odd-indexed ones times
`(1 - z^-1)`, and `a = (P + Q) / 2` truncated to eleven terms. The two helpers `[@ANCHOR: build_half_poly]` (the cascade of
quadratic factors and the boundary factor for one of `P`/`Q`, in `f32`, returning the buffer and its length) and
`[@ANCHOR: poly_mul_fixed]` (plain convolution of two ascending-power polynomials into a fixed-size buffer, returning the
product length; "fixed" refers to the buffer size, not the arithmetic) do the work without heap allocation.

The Q23 decoder path (`lsp_to_lpc_fixed`, used by the fixed `Decoder`) mirrors this in integers.
`[@ANCHOR: lsp_to_lpc_fixed]` computes `cos` of each Q23 angle with `cos_q23`, builds `P` and `Q` with
`build_half_poly_q23`, and returns `(p + q) >> 1` for eleven Q8.23 coefficients.
`[@ANCHOR: cos_q23]` takes an angle in Q23 radians, clamps it to `[0, pi]`, divides by pi (one 128-bit integer division) to
get a position in the 4097-entry table, and linearly interpolates. `[@ANCHOR: cos_lut_table_q23]` is that table:
`cos(pi * i/4096)` in Q23 (generated in `f32`, within two units of exact; exactly 1, 0 and -1 at the start, middle and end).
`[@ANCHOR: build_half_poly_q23]` is the Q8.23 twin of `build_half_poly` (the quadratic factor is `[1, -2cos, 1]` in Q23,
the boundary factor `[1, +-1]`), and `[@ANCHOR: poly_mul_q23]` the Q8.23 twin of `poly_mul_fixed`: each product is formed in
`i128` and shifted right by 23 before it is accumulated.

### Frame energy and bandwidth expansion

`[@ANCHOR: lpc_energy_fixed]` is the `f32` boundary of `lpc_energy_q23`: the sum `a[i] * R[i]` for `i = 0..=10` (the
prediction-error energy that the encoder quantizes) computed in Q23, converted to `f32` by dividing by 2^23.

`[@ANCHOR: apply_bw_gamma_fixed]` multiplies coefficient `a[i]` in place by `0.994^i` (the `BW_GAMMA_Q23` table, Q8.23),
shifting right by 23 (floor). It widens the LPC spectral peaks' bandwidth slightly and must be applied after the energy
is computed, because the energy is defined on the un-expanded coefficients.
