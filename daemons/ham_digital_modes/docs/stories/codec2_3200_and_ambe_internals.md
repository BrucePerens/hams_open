# Story: Codec2 3200 and AMBE Encoder Internals (ham_digital_modes)

These are the low-level building blocks of the Codec2 3200 encoder/decoder (float reference and the
integer-only "fixed" port) and of the TIA-102.BABA AMBE wire layer. Each entry says what the piece does;
the tests that anchor to them compare the fixed port against the float reference or against a captured
real bitstream.

All of the functions in this part of the story are pure computation on integers or floats held in memory. None of them blocks, spawns a
task, takes a lock, reads or writes a file, a socket or hardware, or touches the database. None of them keys a transmitter or looks at a
transmit gate or band check: they only turn audio or bits into other bits or numbers, and whatever sends those bits on the air lives in
the callers' crates. Where an entry has no special concern beyond that, it says so briefly.

## AMBE wire layer

### `encode_code_vectors` [@ANCHOR: ambe_mod:encode_code_vectors]

Turns the eight prioritized bit vectors of one TIA-102.BABA AMBE speech frame into the eight channel code vectors that go on the wire.
The first four vectors are Golay(23,12) coded, the next three are Hamming(15,11) coded, the last is left uncoded, and then the
standard's pseudo-random modulation (Eq. 84-94) is XORed in, keyed by the raw first vector `u[0]`. The float encoder
(`ambe/float/tia_102_baba/mod.rs`) and the fixed-point encoder (`ambe/fixed/tia_102_baba/encode.rs`) both call it as the last step of
building a frame. The DVSI chip's own framing lives in the separate `*_chip` variants.

| Argument | Type | Meaning |
|---|---|---|
| `u` | `[u32; 8]` | The prioritized vectors from `prioritize_bits`: `u[0..=3]` fit in 12 bits, `u[4..=6]` in 11 bits, `u[7]` in 7 bits. |

**Returns:** `[u32; 8]`, the modulated code vectors: four 23-bit Golay words, three 15-bit Hamming words and the 7 raw bits.

**Errors:** none. The function trusts the caller for each vector's bit width and does not re-check it, matching `golay_encode` and
`hamming_encode`; an over-wide vector is silently truncated by the `as u16` cast.

**Special concerns:** pure and allocation free. Modulation is keyed from `u[0]`, so a corrupted `u[0]` corrupts the demodulation of
vectors 1 to 6 at the receiver; the receiver-side inverse is `decode_code_vectors`.

**Signature:** `pub fn encode_code_vectors(u: [u32; 8]) -> [u32; 8]`

**See also:** test `encode_code_vectors_produces_exactly_frame_bits_across_all_eight_vectors`; anchors `ambe_mod:decode_code_vectors`.
*(Reference: `src/ambe/float/tia_102_baba/mod.rs`)*

## Codec2 1600 fixed-point LSP quantiser

### `quantise_dim_fixed` [@ANCHOR: ham_digital_modes:quantise_dim_fixed]

Finds the nearest level of one evenly spaced scalar quantiser dimension for a target frequency, entirely in Q23 `i64` arithmetic so the
Codec2 1600 encoder runs with no floating point. It rounds `(target - start) / step` to the nearest integer using integer division
(add half a step, then truncate) and clamps the index to the valid range. A target at or below `start` maps to level 0. It is called once
per LSP dimension by `encode_lsps_scalar_fixed`.

| Argument | Type | Meaning |
|---|---|---|
| `start_q23` | `i64` | Frequency in Hz (Q23) of level 0 for this dimension. |
| `step_q23` | `i64` | Spacing between levels in Hz (Q23); must be positive, every real step is. |
| `levels` | `u32` | Number of levels; the result is clamped to `0..levels`. Must be at least 1 or the `levels - 1` underflows. |
| `target_hz_q23` | `i64` | The frequency to quantise, in Hz (Q23). |

**Returns:** the level index as `u32`, always below `levels`.

**Errors:** none returned. A zero `step_q23` divides by zero and panics; the real tables never contain one.

**Special concerns:** pure integer code, a genuine integer division being a no-FPU operation. No sign-dependent branch is needed because
the step is always positive and negative differences are handled first.

**Signature:** `fn quantise_dim_fixed(start_q23: i64, step_q23: i64, levels: u32, target_hz_q23: i64) -> u32`

**See also:** test `encode_then_decode_recovers_the_same_quantised_value_for_every_real_index_fixed`.
*(Reference: `src/codec2_1600/lsp_quantiser.rs`)*

## Codec2 3200 model and analysis

### `Model::new` [@ANCHOR: Model::new]

Builds the float harmonic model of one 10 ms sub-frame for a fundamental frequency `wo` and a voicing flag. The harmonic count `l` is
`PI / wo` truncated and capped at `MAX_AMP` (80); the amplitudes `a` start at zero and the phases `phi` at unit real (1 + 0j), ready for
the analysis to fill in. It is compiled only with the `std` feature and is the float reference that `ModelFixed::new` is checked against.

| Argument | Type | Meaning |
|---|---|---|
| `wo` | `f32` | Fundamental frequency in radians per sample. A zero gives `PI/0 = +inf`, which saturates to `MAX_AMP`. |
| `voiced` | `bool` | Whether the sub-frame is voiced. |

**Returns:** a `Model` with `wo`, `l`, `a: [f32; MAX_AMP + 1]` (all 0.0), `phi: [Complex32; MAX_AMP + 1]` (all 1+0j) and `voiced`. Index 0
of the arrays is unused.

**Errors:** none; a NaN `wo` casts to `l = 0`.

**Special concerns:** pure, no allocation (fixed arrays).

**Signature:** `pub fn new(wo: f32, voiced: bool) -> Self`

**See also:** test `model_fixed_new_agrees_with_model_new_within_one_harmonic_on_every_real_wo_index`; anchor `ModelFixed::new`.
*(Reference: `src/codec2_3200/envelope.rs`)*

### `ModelFixed::new` [@ANCHOR: ModelFixed::new]

The Q23 integer twin of `Model::new`, used by the no-float Codec2 3200 path. It computes the harmonic count as `pi_q23() / wo_q23.max(1)`
capped at `MAX_AMP`, so a zero `wo_q23` is treated as one and saturates to the same maximum count the float version reaches through
IEEE infinity. It also precomputes `k_q23 = synth_k_q23(wo)`, the Q23 value of `wo * FFT_ENC / TAU` the synthesis uses to find FFT bins.

| Argument | Type | Meaning |
|---|---|---|
| `wo_q23` | `i64` | Fundamental in radians per sample, Q23. Valid models stay below 2^22 (wo no larger than 2 pi / 20). |
| `voiced` | `bool` | Voicing flag. |

**Returns:** a `ModelFixed` with `wo`, `k_q23`, `l`, `a: [i64; MAX_AMP + 1]` zero, `phi: [ComplexQ23; MAX_AMP + 1]` set to `re = 1 << 23`,
`im = 0`, and `voiced`. The type and constructor are crate-private.

**Errors:** none; a negative `wo_q23` is clamped to 1 by `max(1)` and also saturates.

**Special concerns:** pure integer code with fixed arrays, no float, no allocation.

**Signature:** `pub(crate) fn new(wo_q23: i64, voiced: bool) -> Self`

**See also:** test `model_fixed_new_agrees_with_model_new_within_one_harmonic_on_every_real_wo_index`; anchor `Model::new`.
*(Reference: `src/codec2_3200/envelope.rs`)*

### `Encoder::shift_in` [@ANCHOR: Encoder::shift_in]

Slides the float reference encoder's pitch-analysis history window left by one 10 ms step and appends the new samples as floats. The
window `sn` holds `M_PITCH` (320) samples; `copy_within` moves the last 240 to the front and the newest `N_SAMP` (80) samples are written
at the end. `Encoder::encode` calls it twice per 20 ms frame, once for each 80-sample half, before each analysis sub-step.

| Argument | Type | Meaning |
|---|---|---|
| `new_samples` | `&[i16]` | The newest 80 16-bit samples, oldest first. |

**Returns:** nothing; it mutates `self.sn`.

**Errors:** none. A slice shorter than 80 leaves the unreplaced tail of the window holding the previously shifted values (the `zip` stops
early); a longer slice is truncated.

**Special concerns:** private to the encoder, pure, `&mut self` so one encoder per thread.

**Signature:** `fn shift_in(&mut self, new_samples: &[i16])`

**See also:** anchor `Encoder::encode`; the Codec2 encode/decode tests in `src/codec2_3200/mod.rs`, which cite it.
*(Reference: `src/codec2_3200/floating_reference/mod.rs`)*

### `lsp_dim_value_hz` [@ANCHOR: ham_digital_modes:lsp_dim_value_hz]

Converts one quantiser level of one LSP dimension into the delta frequency it stands for, in Hz. Levels below the dimension's
`breakpoint` are `step1 * (level + 1)`; beyond it the value is `step1 * breakpoint + step2 * (level - breakpoint + 1)`, the second step
size serving the widened dimensions. Both `decode_lsps_delta_scalar` and the float quantiser's nearest-level search use it.

| Argument | Type | Meaning |
|---|---|---|
| `dim` | `&LspDim` | The dimension: `step1` and `step2` (Hz, `f32`) and `breakpoint` (level count, `u32`). `step1 == step2` for a uniform dimension. |
| `level` | `u32` | Quantiser level, 0 to 31 (`LSP_LEVELS` is 32). |

**Returns:** the delta in Hz as `f32` (not yet cumulative; the decoder sums the dimensions).

**Errors:** none; a level above 31 is not rejected and just extends the staircase.

**Special concerns:** compiled with the `std` feature only; pure.

**Signature:** `pub(crate) fn lsp_dim_value_hz(dim: &LspDim, level: u32) -> f32`

**See also:** test `decode_lsps_delta_scalar_fixed_matches_the_float_version_within_quantization_noise_on_real_captured_indices`.
*(Reference: `src/codec2_3200/quantise.rs`)*

### `div_round_i64` [@ANCHOR: ham_digital_modes:div_round_i64]

Integer division with round-to-nearest, halves away from zero, for a positive divisor, in the fixed-point voicing detector. It adds
(or for a negative numerator subtracts) half the divisor before the truncating divide. The detector uses it for the noise-floor
exponential average update, dividing by `NOISE_BETA_DIVISOR` (20, an exact 1/20 for the 0.05 smoothing).

| Argument | Type | Meaning |
|---|---|---|
| `n` | `i64` | Numerator, any sign. |
| `d` | `i64` | Divisor; must be positive. |

**Returns:** `n / d` rounded to nearest as `i64`.

**Errors:** none returned; `debug_assert!(d > 0)` fires in debug builds, and a zero divisor panics in all builds.

**Special concerns:** pure; `n + half` is not overflow checked, fine for the energies it receives.

**Signature:** `fn div_round_i64(n: i64, d: i64) -> i64`

**See also:** test `is_voiced_fixed_matches_is_voiced_on_every_real_scenario_this_module_is_validated_against`; sibling `div_round_i128`.
*(Reference: `src/codec2_3200/voicing.rs`)*

### `div_round_i128` [@ANCHOR: ham_digital_modes:div_round_i128]

The same round-to-nearest divide, half away from zero, widened to `i128` and narrowed to `i64`, for the one true division in the
fixed-point Levinson-Durbin LPC recursion (`k = -(...)/e`). The divisor `e` is always positive by construction, because it starts
positive and is only multiplied by `(1 - k^2) >= 0` with `k` clamped to `[-1, 1]`. `spectral_bridge.rs` also calls it for its regression slope.

| Argument | Type | Meaning |
|---|---|---|
| `n` | `i128` | Numerator, any sign. |
| `d` | `i128` | Divisor; must be positive. |

**Returns:** the rounded quotient as `i64`; the cast truncates if the quotient exceeds `i64`, which the recursion's bounds prevent.

**Errors:** none returned; `debug_assert!(d > 0)` in debug builds, and a zero divisor panics.

**Special concerns:** pure; the narrowing cast is unchecked.

**Signature:** `fn div_round_i128(n: i128, d: i128) -> i64`

**See also:** test `white_noise_correction_measurably_improves_the_worst_case_amplification_margin`; sibling `div_round_i64`.
*(Reference: `src/codec2_3200/lpc.rs`)*

## Codec2 3200 pitch estimator (nlp)

### `design_lowpass` [@ANCHOR: ham_digital_modes:design_lowpass]

Designs the anti-alias low-pass FIR used before the pitch estimator decimates its input by `NLP_DEC` (5). It builds a 25-tap
windowed-sinc filter, a Hann window times the sinc of the given cutoff, centred on the middle tap, then divides every tap by the sum so
the DC gain is exactly 1. The float reference filter is built once from it (`0.5 / NLP_DEC` cycles per sample, so 0.1) and cached; the
fixed path quantises the same taps to Q23.

| Argument | Type | Meaning |
|---|---|---|
| `taps` | `usize` | Filter length; must equal `LPF_TAPS` (25) or the fixed array index and the Hann denominator go wrong (a value above 25 panics). |
| `cutoff` | `f32` | Cutoff in cycles per original-rate sample. |

**Returns:** `[f32; 25]` of coefficients, symmetric, summing to 1.0.

**Errors:** none returned.

**Special concerns:** `std` feature only; computed once, not per frame, so its float trigonometry is off the real-time path.

**Signature:** `pub(crate) fn design_lowpass(taps: usize, cutoff: f32) -> [f32; LPF_TAPS]`

**See also:** test `decimate_fixed_matches_the_float_decimate_on_realistic_amplitude_input`; anchors `lowpass_coeffs`, `decimate`.
*(Reference: `src/codec2_3200/nlp.rs`)*

### `dc_notch_fixed` [@ANCHOR: ham_digital_modes:dc_notch_fixed]

One step of the first-order DC-blocking notch the pitch estimator applies to the squared speech samples, in integers. The output is
`y = x - mem_x + (a * mem_y) >> 23` rounded, with both memories updated. Only the notch coefficient `a` (`NLP_NOTCH_A_Q23`) is Q23; the
samples are plain squared-sample magnitudes, descaled after the multiply. `a * mem_y` fits `i64` because `a` is at most 2^23 and a squared
`i16` sample is at most about 1.07e9.

| Argument | Type | Meaning |
|---|---|---|
| `x` | `i64` | The current squared sample. |
| `mem_x` | `&mut i64` | Previous input; set to `x`. |
| `mem_y` | `&mut i64` | Previous output; set to the result. |

**Returns:** the notched value `y` as `i64`.

**Errors:** none.

**Special concerns:** state lives in the caller's per-stream estimator (`mem_x_fixed`, `mem_y_fixed`), so one state per stream and no sharing between threads.

**Signature:** `fn dc_notch_fixed(x: i64, mem_x: &mut i64, mem_y: &mut i64) -> i64`

**See also:** test `dc_notch_fixed_matches_the_float_dc_notch_on_realistic_amplitude_input`.
*(Reference: `src/codec2_3200/nlp.rs`)*

### `rshift_round_i128` [@ANCHOR: nlp:rshift_round_i128]

A rounding right shift of an `i128` accumulator that narrows the result to `i64` and `debug_assert!`s that it fits. The `nlp:` prefix
keeps it distinct from `fixed_fft.rs`, which defines its own same-named function with its own callers. In `nlp.rs` this version is
compiled only under `cfg(test)`, where the decimator and FFT tests use it as the exact reference; production code that needs a result
wider than `i64` uses `rshift_round_i128_wide` instead.

| Argument | Type | Meaning |
|---|---|---|
| `x` | `i128` | The accumulator. |
| `n` | `u32` | Shift count, at least 1 (the rounding constant is `1 << (n - 1)`). |

**Returns:** `(x + 2^(n-1)) >> n` as `i64`.

**Errors:** the `debug_assert!` fires in debug builds when the result does not fit `i64`, with a message pointing at the wide variant; in
release the cast silently wraps.

**Special concerns:** pure; test-only in this file.

**Signature:** `fn rshift_round_i128(x: i128, n: u32) -> i64`

**See also:** tests `decimate_fixed_matches_the_float_decimate_on_realistic_amplitude_input`, `fft_fixed_power_spectrum_matches_a_float_fft_on_a_multi_tone_input`.
*(Reference: `src/codec2_3200/nlp.rs`)*

### `rshift_round_i128_wide` [@ANCHOR: ham_digital_modes:rshift_round_i128_wide]

The same rounding right shift, but it keeps the full `i128` width. It exists for the sub-multiple threshold in `correct_sub_multiples_fixed`,
`CNLP * gmax`, which can exceed `i64::MAX` at full-scale input and once silently wrapped, corrupting the pitch decision. The name is unique
in the repository, so its anchor needs no prefix.

| Argument | Type | Meaning |
|---|---|---|
| `x` | `i128` | The accumulator, such as `cnlp * gmax`. |
| `n` | `u32` | Shift count, at least 1. |

**Returns:** `(x + 2^(n-1)) >> n` as `i128`.

**Errors:** none; `n` of 0 underflows the shift and panics in debug builds.

**Special concerns:** pure.

**Signature:** `fn rshift_round_i128_wide(x: i128, n: u32) -> i128`

**See also:** test `nlp_fixed_agrees_with_the_float_reference_at_full_scale_amplitude`, which fails if the narrowing version is used there.
*(Reference: `src/codec2_3200/nlp.rs`)*

## Codec2 3200 fixed-point FFT arithmetic helpers (fixed_fft)

The integer FFT keeps every butterfly exact while avoiding 128-bit arithmetic whenever the values are small enough, so a few
small helpers choose between equivalent forms; each is tested against the plain exact `i128` expression. All are private to
`src/codec2_3200/fixed_fft.rs` except `mag_sq_raw`, which is `pub(crate)`, and none takes a lock or does I/O.

### `ComplexQ23::mag_sq_raw` [@ANCHOR: fixed_fft:ComplexQ23_mag_sq_raw]

The squared magnitude `re^2 + im^2` of a Q23 complex value in Q46, left unshifted. It is computed with `u64` arithmetic when both parts are
under 2^31 in magnitude (each square then stays under 2^62 and the sum under 2^63) and with `i128` otherwise; the two paths return the
same number. Its caller in the envelope code, `mag_sq_q23`, shifts the result back to Q23.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `ComplexQ23` | Value with `re` and `im` as `i64` Q23. |

**Returns:** `i128`, the exact sum of squares.

**Errors:** none; exact for every `i64` pair.

**Special concerns:** pure; the fast path avoids a 128-bit multiply that is slow on 32-bit cores.

**Signature:** `pub(crate) fn mag_sq_raw(self) -> i128`

**See also:** test `complex_mul_and_mag_sq_fast_paths_match_the_i128_forms`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `twiddle_mul` [@ANCHOR: fixed_fft:twiddle_mul]

Multiplies a Q23 twiddle factor `(wr, wi)` by a complex value `(br, bi)` and rounds back to Q23, the core multiply of a radix-2 butterfly.
When both parts of the operand are under 2^38 in magnitude the products fit `i64` (under 2^62) and are rounded in `i64`; otherwise the exact
`i128` form runs. The result is bit-identical either way. The `i128` path is kept because decoding pseudo-random garbage bitstreams was
measured reaching 2^67 in a butterfly.

| Argument | Type | Meaning |
|---|---|---|
| `wr`, `wi` | `i64` | Twiddle real and imaginary parts, Q23 (magnitude at most 2^23). |
| `br`, `bi` | `i64` | The complex operand. |

**Returns:** `(re, im)` of `w * b`, rounded with `(x + 2^22) >> 23`.

**Errors:** none.

**Special concerns:** `#[inline(always)]`, called per butterfly from the generic `bf`, so it is on the real-time path; pure.

**Signature:** `fn twiddle_mul(wr: i64, wi: i64, br: i64, bi: i64) -> (i64, i64)`

**See also:** test `twiddle_mul_matches_the_exact_i128_product_either_side_of_the_i64_switch`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `mode_for_sum` [@ANCHOR: fixed_fft:mode_for_sum]

Picks the kernel for one transform from the sum of the absolute values of its input components. Below `I32_SUM_LIMIT` (2^29) every
intermediate fits 32 bits and it returns `MODE_I32`; below `I64_SUM_LIMIT` (2^39) it returns `I64_KERNEL`, which is the native-64-bit
form on 64-bit targets and the split 32-bit-halves form on 32-bit ones; above that it returns `MODE_CHECKED`, the exact checked path.

| Argument | Type | Meaning |
|---|---|---|
| `sum` | `u64` | Sum of the magnitudes of the input values, from `sum_abs`. |

**Returns:** a `u8` mode constant. The bound on the 64-bit limit is `|w| * |b|` of at most 2^23 times sqrt(2) * 2^39, under 2^62.6.

**Errors:** none; every `sum` maps to a mode.

**Special concerns:** called once per transform (`fft_fixed` and the sparse variants), so a wrong threshold would silently overflow; the
test pins the limits exactly.

**Signature:** `fn mode_for_sum(sum: u64) -> u8`

**See also:** test `mode_for_sum_switches_at_exactly_the_documented_limits`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `mul_w_x` [@ANCHOR: fixed_fft:mul_w_x]

Multiplies a 24-bit signed twiddle `w` by an `i64` value `x` using only 32-bit multiplies, for targets with no 64-bit multiplier. It forms
the low 64 bits of the product as two 32-bit words: one signed-by-unsigned 32x32 product of `w` with the low word of `x`, plus the cross
term `w * high(x)` (wrapping), because the part of that product above bit 64 is discarded anyway. The pair equals the plain 64-bit product.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `i32` | Twiddle, 24-bit signed. |
| `x` | `i64` | Value to multiply. |

**Returns:** `(low: u32, high: i32)` words of `w * x` truncated to 64 bits.

**Errors:** none; the result is only meaningful when the true product fits `i64`.

**Special concerns:** pure.

**Signature:** `fn mul_w_x(w: i32, x: i64) -> (u32, i32)`

**See also:** test `split_word_multiplies_equal_the_plain_64_bit_arithmetic`; used by `mul_round_i64`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `umul_x` [@ANCHOR: fixed_fft:umul_x]

Multiplies an unsigned 24-bit constant `c` by an `i64` value `x` into the low 64 bits of the product, as (low, high) 32-bit words: one full
32x32 to 64 unsigned product for the low word of `x` and one 32-bit product for its high word. Because `c` is unsigned no sign
corrections are needed, which is why the inverse transform, whose twiddles it can arrange to be positive magnitudes, uses it.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `u32` | Unsigned multiplier, under 2^24. |
| `x` | `i64` | Value; its two's-complement bits are used. |

**Returns:** `(low: u32, high: u32)`.

**Errors:** none.

**Special concerns:** pure; the pair is the two's-complement 64-bit product, so interpret `high` as signed when a signed result is wanted.

**Signature:** `fn umul_x(c: u32, x: i64) -> (u32, u32)`

**See also:** test `split_word_multiplies_equal_the_plain_64_bit_arithmetic`; used by `twiddle_sums`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `mul_round_i64` [@ANCHOR: fixed_fft:mul_round_i64]

Computes `(w1*x1 + w2*x2 + 2^22) >> 23` on 32-bit halves: the Q23-rounded two-term sum the generic butterfly `bf` needs in the 32-bit-halves mode, for 24-bit twiddles
`w1`, `w2` and `i64` values whose weighted sum is under 2^62. It calls `mul_w_x` twice, adds the pairs with carry, adds the rounding
constant, and shifts the 64-bit pair right by 23 (an arithmetic shift). It is bit-identical to the plain `i64` expression.

| Argument | Type | Meaning |
|---|---|---|
| `w1`, `w2` | `i32` | 24-bit signed twiddle components. |
| `x1`, `x2` | `i64` | Values whose weighted sum magnitude is under 2^62. |

**Returns:** the rounded sum as `i64`.

**Errors:** none; outside the documented bound the result wraps.

**Special concerns:** pure; exists only to avoid 64-bit multiplies on RV32 and Cortex-M class cores.

**Signature:** `fn mul_round_i64(w1: i32, x1: i64, w2: i32, x2: i64) -> i64`

**See also:** test `split_word_multiplies_equal_the_plain_64_bit_arithmetic`; anchors `fixed_fft:mul_w_x`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `twiddle_sums` [@ANCHOR: fixed_fft:twiddle_sums]

Returns the two exact 64-bit sums of an inverse-transform twiddle multiply with first-quadrant twiddle `(c, -s)`, where `c` and `s` are
positive: `A = c*x - s*y` and `B = c*y + s*x`, the real and imaginary parts of `w * (x + j y)` before rounding, each as a (low, high)
pair of 32-bit words. It uses four `umul_x` calls and carries by hand; the caller `block_inv_i64` then rounds
the pairs with `round_shift` or `round_shift_neg`.

| Argument | Type | Meaning |
|---|---|---|
| `c`, `s` | `u32` | Unsigned cosine and sine magnitudes of the twiddle, Q23. |
| `x`, `y` | `i64` | Real and imaginary parts of the data value. |

**Returns:** `((al, ah), (bl, bh))`, the low and high words of `A` and `B`.

**Errors:** none; the result is the true sum modulo 2^64.

**Special concerns:** pure, `#[inline(always)]`.

**Signature:** `fn twiddle_sums(c: u32, s: u32, x: i64, y: i64) -> ((u32, u32), (u32, u32))`

**See also:** test `twiddle_sums_and_the_rounded_shifts_equal_the_exact_i128_values`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `round_shift` [@ANCHOR: fixed_fft:round_shift]

Turns a 64-bit value held as (low, high) 32-bit words into its Q23-rounded value `(v + 2^22) >> 23` as an `i64`, using an arithmetic
shift. It adds the rounding constant to the low word with carry into the high word, then shifts the pair. It is the finishing step after
`twiddle_sums` for the real and imaginary outputs of an inverse butterfly.

| Argument | Type | Meaning |
|---|---|---|
| `(lo, hi)` | `(u32, u32)` | Low and high words of the 64-bit value `v`. |

**Returns:** `(v + 2^22) >> 23` as `i64`.

**Errors:** none; the result must fit `i64` or wraps, which the caller's magnitude bounds guarantee.

**Special concerns:** pure.

**Signature:** `fn round_shift((lo, hi): (u32, u32)) -> i64`

**See also:** test `twiddle_sums_and_the_rounded_shifts_equal_the_exact_i128_values`; sibling `round_shift_neg`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

### `round_shift_neg` [@ANCHOR: fixed_fft:round_shift_neg]

The rounded Q23 shift of the negation of a 64-bit (low, high) value: `(2^22 - v) >> 23`. It saves the separate negate that would otherwise
precede `round_shift`; the inverse butterfly needs `-B` for the real output of the upper-half twiddles, so it passes the sum pair
straight in. The subtraction borrows across the two words by hand.

| Argument | Type | Meaning |
|---|---|---|
| `(lo, hi)` | `(u32, u32)` | Low and high words of the 64-bit value `v`. |

**Returns:** the rounded shift of `-v` as `i64`.

**Errors:** none.

**Special concerns:** pure. The edge case `v = i64::MIN` negates to itself in two's complement, outside the intended range.

**Signature:** `fn round_shift_neg((lo, hi): (u32, u32)) -> i64`

**See also:** test `twiddle_sums_and_the_rounded_shifts_equal_the_exact_i128_values`; sibling `round_shift`.
*(Reference: `src/codec2_3200/fixed_fft.rs`)*

## Codec2 3200 small integer helpers (fixed_point, nlp, envelope, synthesis)

### `clz_nonzero` [@ANCHOR: ham_digital_modes:clz_nonzero]

Counts the leading zero bits of a nonzero `u32` with two range steps and a 256-byte table (`CLZ8`). A core without a count-leading-zeros
instruction (RV32IMC, Cortex-M0 or M3 class) expands `leading_zeros` into a much longer sequence, and `log2_q23` runs it about a
thousand times per frame. The first step tests for 16 leading zero bits and shifts, the second for 8, and the table finishes the top byte.

| Argument | Type | Meaning |
|---|---|---|
| `x` | `u32` | Value; must be nonzero (zero is outside the contract, and `log2_q23` never passes it). |

**Returns:** the count of leading zero bits, 0 to 31.

**Errors:** none; it must equal `leading_zeros` for every nonzero input.

**Special concerns:** `#[inline(always)]`, pure.

**Signature:** `fn clz_nonzero(x: u32) -> u32`

**See also:** test `clz_nonzero_equals_leading_zeros_for_nonzero_values`.
*(Reference: `src/codec2_3200/fixed_point.rs`)*

### `table_is_non_decreasing` [@ANCHOR: ham_digital_modes:table_is_non_decreasing]

A `const fn` that returns whether a lookup table never steps down. Compile-time `const _: () = assert!(...)` lines apply it to
`LOG2_LUT_Q23` and `EXP2_LUT_FRAC_Q23` so the build fails if either interpolation table is ever edited to a non-monotone state; the
interpolation arithmetic in `log2_q23` and its inverse assumes `t1 - t0` is never negative.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&[i32; LOG2_LUT_SIZE]` | The table, `LOG2_LUT_SIZE` = 257 entries (`LOG2_LUT_BITS` 8). |

**Returns:** `true` when every entry is at least its predecessor.

**Errors:** none; the failure mode is a compile error at the assertion sites.

**Special concerns:** runs only at compile time and in its test.

**Signature:** `const fn table_is_non_decreasing(t: &[i32; LOG2_LUT_SIZE]) -> bool`

**See also:** test `table_is_non_decreasing_accepts_the_real_tables_and_rejects_one_step_down`.
*(Reference: `src/codec2_3200/fixed_point.rs`)*

### `window_fits_i32` [@ANCHOR: ham_digital_modes:window_fits_i32]

A `const fn` that checks that every entry of an `i64` window table survives a round trip through `i32`. A compile-time assertion applies it
to `NLP_LOWPASS_Q23`, which lets `decimate_fixed` use 32-bit kernels whose 32x32 to 64 products are only valid when the coefficients
fit 32 bits. An empty slice returns true.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&[i64]` | The table to check. |

**Returns:** `true` when `w[i] as i32 as i64 == w[i]` for every `i`.

**Errors:** none; failure shows as a compile error at the assertion.

**Special concerns:** compile time only.

**Signature:** `const fn window_fits_i32(w: &[i64]) -> bool`

**See also:** test `window_fits_i32_is_true_only_when_every_entry_fits`.
*(Reference: `src/codec2_3200/nlp.rs`)*

### `window_is_bounded` [@ANCHOR: ham_digital_modes:window_is_bounded]

A `const fn` that checks that every entry of a Q23 window table lies within plus or minus 1.0 (`1 << 23`). Compile-time assertions apply it
to `SYNTH_PARZEN_Q23` and, with the `codec2_16k_bridge` feature, `SB_PARZEN_Q23`; the guarantee is what lets `mul_q23` bound its product
and what keeps the synthesis overlap-add from overflowing.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&[i64]` | The window table, Q23. |

**Returns:** `true` when no entry exceeds 1.0 in magnitude.

**Errors:** none; failure is a compile error.

**Special concerns:** compile time only.

**Signature:** `const fn window_is_bounded(w: &[i64]) -> bool`

**See also:** test `window_is_bounded_accepts_the_q23_unit_range_and_rejects_beyond`; anchor `ham_digital_modes:mul_q23`.
*(Reference: `src/codec2_3200/synthesis.rs`)*

### `square_words` [@ANCHOR: ham_digital_modes:square_words]

Squares a 64-bit magnitude into two 64-bit words (low, high), the full 128-bit square, from three 32x32 to 64 products. A core with only
32-bit multiplies cannot do better, and the compiler's own 128-bit multiply is several times longer. With `x = h*2^32 + l`, the square is
`l*l + 2^33 * h*l + 2^64 * h*h`; the low word takes the carry of the middle term. `power_bin_split` calls it.

| Argument | Type | Meaning |
|---|---|---|
| `x` | `u64` | Magnitude (callers pass `unsigned_abs()` of an `i64`). |

**Returns:** `(low, high)` words of `x * x`.

**Errors:** none; exact for every `u64`.

**Special concerns:** pure, `#[inline(always)]`.

**Signature:** `fn square_words(x: u64) -> (u64, u64)`

**See also:** test `square_words_and_power_bin_are_the_exact_wide_squares`; anchor `ham_digital_modes:power_bin`.
*(Reference: `src/codec2_3200/nlp.rs`)*

### `power_bin` [@ANCHOR: ham_digital_modes:power_bin]

The pitch estimator's power of one spectrum bin, `re^2 + im^2`, as an exact `i128`. It dispatches on `NATIVE_64_BIT`: 64-bit targets
use `power_bin_native` (plain 128-bit products) and 32-bit targets use `power_bin_split`, which builds each square from 32-bit limbs
with `square_words`. Both forms are compiled everywhere and tested against each other.

| Argument | Type | Meaning |
|---|---|---|
| `re`, `im` | `i64` | Real and imaginary parts of the bin, as left by the integer FFT. |

**Returns:** the exact sum of squares as `i128`.

**Errors:** none.

**Special concerns:** pure; the dispatch is a compile-time constant, so there is no run-time branch cost.

**Signature:** `fn power_bin(re: i64, im: i64) -> i128`

**See also:** test `square_words_and_power_bin_are_the_exact_wide_squares`.
*(Reference: `src/codec2_3200/nlp.rs`)*

### `mag_sq_q23` [@ANCHOR: ham_digital_modes:mag_sq_q23]

The squared magnitude of one LPC-spectrum bin, `re^2 + im^2`, rounded to Q23, on whichever of three paths fits the operands. If both
parts fit `i32` it uses two signed 32x32 to 64 squares; else, if both are under 2^31, unsigned 32-bit squares in `u64`; otherwise it
falls back to `ComplexQ23::mag_sq_raw` and an `i128` rounded shift. A transform of real LPC coefficients always lands in the first path.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `ComplexQ23` | A bin of the LPC spectrum, `re` and `im` as `i64` Q23. |

**Returns:** the squared magnitude in Q23 as `i64`, rounded to nearest.

**Errors:** none; the three paths give the same value (the test checks all).

**Special concerns:** private to `envelope.rs`, pure.

**Signature:** `fn mag_sq_q23(c: ComplexQ23) -> i64`

**See also:** test `mag_sq_q23_equals_the_exact_rounded_square_on_every_path`.
*(Reference: `src/codec2_3200/envelope.rs`)*

### `mul_q23` [@ANCHOR: ham_digital_modes:mul_q23]

Multiplies a transform output `x` by a Q23 window value `w` and shifts back: `(x * w) >> 23`. `w` is at most 2^23 in magnitude, checked at
compile time for both windows by `window_is_bounded`, so when `|x|` is under 2^40 the product stays under 2^63 and plain `i64` is
used; larger values widen to `i128`. The synthesis overlap-add calls it per sample.

| Argument | Type | Meaning |
|---|---|---|
| `x` | `i64` | Transform output sample. |
| `w` | `i64` | Window value, Q23, magnitude at most `1 << 23`. |

**Returns:** the product shifted right by 23 (arithmetic, floor) as `i64`.

**Errors:** none; the two paths are equal where both are valid.

**Special concerns:** pure, `#[inline(always)]`, on the real-time path.

**Signature:** `fn mul_q23(x: i64, w: i64) -> i64`

**See also:** test `mul_q23_equals_the_exact_product_either_side_of_the_i64_switch`; anchor `ham_digital_modes:window_is_bounded`.
*(Reference: `src/codec2_3200/synthesis.rs`)*

## Small helpers anchored in slice 3 (wire layout, FEC, tables, tracking, header packing, timing)

### `natural_range` [@ANCHOR: ham_digital_modes:natural_range]

Gives the `(start, length)` of one block of the 144-bit P25 chip wire frame in natural (pre-interleave) order: four 23-bit Golay blocks at
offsets 0, 23, 46 and 69, three 15-bit Hamming blocks at 92, 107 and 122, and the 7 raw bits at 137. Together they tile the frame
exactly once. `block_wire_members` in `wire_format.rs` uses it to locate each block's bits.

| Argument | Type | Meaning |
|---|---|---|
| `block` | `Block` | `Golay { index }` with index 0 to 3, `Hamming { index }` with index 0 to 2, or `Raw`. |

**Returns:** `(usize, usize)`, the start bit and the length.

**Errors:** an index past the last block of its kind is a programming error and panics through `assert!` (usable in `const` context, so it
can fail compilation).

**Special concerns:** pure; the panic is deliberate so a bad index never turns into an out-of-bounds read later.

**Signature:** `const fn natural_range(block: Block) -> (usize, usize)`

**See also:** tests `natural_range_tiles_the_144_bit_frame_exactly_once`, `natural_range_refuses_a_golay_index_past_the_last_block`.
*(Reference: `src/ambe/dvsi_p25fec/wire_format.rs`)*

### `hamming_decode_chip` [@ANCHOR: ham_digital_modes:hamming_decode_chip]

Minimum-distance decoder of the DVSI chip's Hamming(15,11) variant, the inverse of `hamming_encode_chip`. It masks the input to 15 bits
(so bit 15 is ignored), tries all 2048 data words, encodes each with the chip's own parity table and keeps the one closest to the received
word. It recovers every data word exactly and corrects any single flipped bit. It is used by `decode_code_vectors_chip` and by `decode_block` in the chip FEC module.

| Argument | Type | Meaning |
|---|---|---|
| `received` | `u16` | The received 15-bit codeword; bit 15 is masked off. |

**Returns:** `(data, distance)`: the 11-bit data word and the Hamming distance to the chosen codeword, which doubles as the corrected-error
count. Ties keep the lowest data value.

**Errors:** none; an uncorrectable word still returns the nearest codeword with a distance of 2 or more.

**Special concerns:** brute force, 2048 encodes per call, so it is a test and tooling path rather than a hot loop.

**Signature:** `pub fn hamming_decode_chip(received: u16) -> (u16, u32)`

**See also:** test `hamming_decode_chip_recovers_every_data_word_and_corrects_one_flipped_bit`; anchor `ham_digital_modes:dvsi_p25fec_fec_hamming_encode_chip`.
*(Reference: `src/ambe/dvsi_p25fec/fec.rs`)*

### `gain_vector_bits` [@ANCHOR: ham_digital_modes:gain_vector_bits]

The integer-only `bits` half of the TIA-102.BABA gain bit-allocation table: how many bits the quantiser spends on gain element `m` for
a frame with `L` harmonics. It exists so the fixed-point reconstruct and quantize modules can read the bit count without calling any
function that builds an `f64`, even transiently. It indexes the same `GAIN_BIT_ALLOCATION` table as the full float accessor.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `u32` | Harmonic count, valid 9 to 56. |
| `m` | `u32` | Gain element number, valid 2 to 6. |

**Returns:** `Some(bits)` as `u8`, or `None` when `l` or `m` is outside the valid range.

**Errors:** `None` is the only failure signal; callers (`reconstruct_gain_vector_q16`, the quantiser) propagate it with `?`.

**Special concerns:** pure, no float.

**Signature:** `pub fn gain_vector_bits(l: u32, m: u32) -> Option<u8>`

**See also:** test `gain_vector_bits_agrees_with_the_full_allocation_table_and_refuses_out_of_range`.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs`)*

### `tracking_range` [@ANCHOR: ham_digital_modes:tracking_range]

Returns the contiguous run of pitch candidate indices whose pitch lies within 0.8 to 1.2 of a reference pitch, as the pitch-tracking
window of the fixed-point TIA-102 analysis. The test is exact: pitches are compared as half samples (`p2_of_index`) with the integer
inequality `4*r2 <= 5*c2 <= 6*r2`, so no rounding error moves a candidate in or out. `look_ahead_pitch_tracking` and `look_back_pitch_tracking` both iterate it.

| Argument | Type | Meaning |
|---|---|---|
| `ref_idx` | `usize` | Index of the reference pitch among the `CANDIDATES` (203) candidates. |

**Returns:** `RangeInclusive<usize>` of qualifying indices. It is never empty for a valid index, since the reference itself qualifies.

**Errors:** none.

**Special concerns:** scans all 203 candidates per call; pure.

**Signature:** `fn tracking_range(ref_idx: usize) -> std::ops::RangeInclusive<usize>`

**See also:** test `tracking_range_is_the_exact_eighty_to_one_twenty_percent_window`.
*(Reference: `src/ambe/fixed/tia_102_baba/pitch.rs`)*

### `ratio_q30` [@ANCHOR: ham_digital_modes:ratio_q30]

Divides two wide Q60 energies into a Q30 ratio `error / real` for the fixed-point voicing measure. It shifts both energies right by the
same amount (the number of bits of `real` above 90) so the numerator, shifted left by 30, cannot overflow `i128`, then divides and clamps to
`i64`. If `real` shifts down to zero it returns `ONE_Q30` (1.0). `voicing_measure_q30` calls it with a band's error and real energies.

| Argument | Type | Meaning |
|---|---|---|
| `err` | `i128` | Non-negative error energy, Q60. |
| `real` | `i128` | Non-negative real energy, Q60; must be positive for a meaningful result. |

**Returns:** the ratio as Q30 `i64`.

**Errors:** none returned; a negative `real` would give a meaningless value, and the shift of `bits` assumes non-negative inputs.

**Special concerns:** pure; shifting both operands loses low bits, acceptable for a ratio.

**Signature:** `fn ratio_q30(err: i128, real: i128) -> i64`

**See also:** test `ratio_q30_is_the_q30_quotient_and_survives_very_wide_energies`.
*(Reference: `src/ambe/fixed/tia_102_baba/vuv.rs`)*

### `DStarHeader::pack_field` [@ANCHOR: ham_digital_modes:DStarHeader_pack_field]

Writes one callsign field of a D-STAR header into an output buffer, space-padded on the right to the field's wire width. It is the helper
behind `DStarHeader::pack`, which calls it for the rpt2, rpt1, ur_call, my_call (8 bytes each) and my_call_suffix (4 bytes) fields. It refuses non-ASCII text and an overlong value, and in both cases appends nothing to the buffer.

| Argument | Type | Meaning |
|---|---|---|
| `value` | `&str` | The field text without padding. |
| `width` | `usize` | The fixed wire width in bytes. |
| `out` | `&mut Vec<u8>` | The header body being built; bytes are appended. |

**Returns:** `Ok(())` after appending exactly `width` bytes.

**Errors:** `DStarHeaderError::NonAscii` when `value` has a non-ASCII character; `DStarHeaderError::FieldTooLong` when it is longer than `width`.

**Special concerns:** pure apart from appending to `out`. A header that fails to pack is never sent, so a malformed callsign cannot reach
the transmit path; the check does not validate the callsign's content, only its encoding and length.

**Signature:** `fn pack_field(value: &str, width: usize, out: &mut Vec<u8>) -> Result<(), DStarHeaderError>`

**See also:** test `pack_field_pads_with_spaces_and_refuses_non_ascii_or_overlong_text`; anchor `DStarHeader::pack`.
*(Reference: `src/dstar.rs`)*

### `two_tone_window_len` [@ANCHOR: ham_digital_modes:two_tone_window_len]

Chooses the analysis window length, in samples, for the two-tone timing feature of the RTTY timing characteriser. The window must span at
least one cycle of the shift frequency (`ceil(sample_rate / shift_hz)`) and be shorter than one symbol at the fastest baud rate
searched (`floor(sample_rate / max_baud)`, exclusive), so that its own geometry artifact cannot land in the baud range and pass as an
answer. It returns the midpoint of that band.

| Argument | Type | Meaning |
|---|---|---|
| `sample_rate` | `u32` | Samples per second. |
| `shift_hz` | `f64` | Mark/space shift, for example 170.0. |
| `max_baud` | `f64` | Highest baud rate the search covers, for example 100.0. |

**Returns:** `Some(len)` (for 48000, 170, 100 the band is 283 to 480 and the result 381), or `None` when no window satisfies both limits.

**Errors:** `None` means this configuration cannot be characterised this way; callers must not force a window anyway.

**Special concerns:** pure; zero `shift_hz` or `max_baud` gives an infinite float and a meaningless usize, so callers pass real values.

**Signature:** `pub fn two_tone_window_len(sample_rate: u32, shift_hz: f64, max_baud: f64) -> Option<usize>`

**See also:** test `two_tone_window_len_is_the_midpoint_of_the_valid_band_or_none`; sibling `two_tone_hop_len`.
*(Reference: `src/timing_characterizer.rs`)*

## Fixed-point AMBE helpers anchored in slice 4 (noise, twiddles, dequantizer, blocks)

### `NoiseState::new` [@ANCHOR: ham_digital_modes:NoiseState_new]

Creates the integer noise generator of the AMBE unvoiced synthesis (Eq. 117, `u(n+1) = (171 u(n) + 11213) mod 53125`) at its
documented start, `u(-105) = 3147`, then runs it 209 steps so the window already holds `u(-104..=104)` for the first frame. The state
is a `last` value and a 209-entry `VecDeque` window. Each synthesiser owns one, so the noise is deterministic from stream start.

**Returns:** a `NoiseState` with a full 209-sample window.

**Errors:** none.

**Special concerns:** allocates the deque once; not shared between threads, one per decoder stream.

**Signature:** `pub fn new() -> Self` (also `Default`)

**See also:** test `noise_state_follows_the_published_recurrence_one_frame_at_a_time`; anchors `NoiseState_step`, `NoiseState_at`.
*(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs`)*

### `NoiseState::step` [@ANCHOR: ham_digital_modes:NoiseState_step]

Advances the noise generator by one sample: `last = advance_noise(last)` (the Eq. 117 recurrence, `rem_euclid(53125)`), then pops the oldest
value if the window already holds 209 and pushes the new one. It is private; `new` and `advance_frame` are its only callers.

**Returns:** nothing; mutates `last` and the window.

**Errors:** none.

**Special concerns:** pure integer, constant work per call, one deque push and pop.

**Signature:** `fn step(&mut self)`

**See also:** test `noise_state_follows_the_published_recurrence_one_frame_at_a_time`; anchor `NoiseState_advance_frame`.
*(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs`)*

### `NoiseState::advance_frame` [@ANCHOR: ham_digital_modes:NoiseState_advance_frame]

Moves the noise window on by one speech frame, `N` = 160 steps, so the next frame's `n = 0` sits 160 samples later in the sequence. The fixed synthesis state calls it once per frame, which keeps the noise continuous across frame boundaries as the
standard requires.

**Returns:** nothing.

**Errors:** none.

**Special concerns:** 160 deque operations per frame; pure integer.

**Signature:** `pub fn advance_frame(&mut self)`

**See also:** test `noise_state_follows_the_published_recurrence_one_frame_at_a_time`.
*(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs`)*

### `NoiseState::at` [@ANCHOR: ham_digital_modes:NoiseState_at]

Reads the noise sample `u(relative)` relative to the current frame's `n = 0`. The window holds `-104..=104`, which covers the 209 samples
the 256-point DFT of the windowed noise and the voiced phase dither need. Anything outside that
range is refused rather than extrapolated.

| Argument | Type | Meaning |
|---|---|---|
| `relative` | `i32` | Offset from the frame's `n = 0`, valid -104 to 104. |

**Returns:** `Some(u)` as `i64` in `0..53125`, or `None` if `relative` is out of range.

**Errors:** `None` outside `-104..=104`. `phase_dither_q16` unwraps it with `expect`, since harmonic numbers 1 to 56 are always inside.

**Special concerns:** read only, `&self`.

**Signature:** `pub fn at(&self, relative: i32) -> Option<i64>`

**See also:** test `noise_state_follows_the_published_recurrence_one_frame_at_a_time`.
*(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs`)*

### `ComplexQ16::norm_sqr_q16` [@ANCHOR: ham_digital_modes:ComplexQ16_norm_sqr_q16]

The squared magnitude of an `i64` Q16.16 complex spectral value in Q16.16, computed through an `i128` intermediate so a large bin cannot
overflow: `(re*re + im*im) >> 16`. `unvoiced_spectrum_q16` uses it to get the power of the noise-spectrum bins around each harmonic before rescaling to the decoded amplitude.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `ComplexQ16` | `re`, `im` as `i64` Q16.16. |

**Returns:** the power in Q16.16 as `i64` (truncating shift; the cast narrows).

**Errors:** none; a power above `i64` after the shift would wrap, which real bins do not reach.

**Special concerns:** pure.

**Signature:** `fn norm_sqr_q16(self) -> i64`

**See also:** test `norm_sqr_q16_is_the_squared_magnitude_without_overflow`.
*(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs`)*

### `mul_div_wide` [@ANCHOR: ham_digital_modes:mul_div_wide]

Computes `a * b / c` for three Q16.16 `i64` values as one division of a widened `i128` product. `unvoiced_spectrum_q16` uses it to rescale a
spectral bin by `gamma_w * M_l / sqrt(power)` without forming that ratio alone, which would be too coarse for a quiet harmonic, and
without narrowing `b` to `i32` (its `gm_q16` often does not fit). Dividing once also rounds once instead of twice.

| Argument | Type | Meaning |
|---|---|---|
| `a_q16`, `b_q16` | `i64` | The two factors, Q16.16. |
| `c_q16` | `i64` | The divisor, Q16.16. |

**Returns:** `a * b / c` as `i64` (truncating toward zero); the Q16 scales cancel to leave a Q16.16 result.

**Errors:** a zero divisor returns 0 rather than panicking.

**Special concerns:** pure.

**Signature:** `fn mul_div_wide(a_q16: i64, b_q16: i64, c_q16: i64) -> i64`

**See also:** test `mul_div_wide_divides_the_widened_product_once`.
*(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs`)*

### `twiddle_q16` [@ANCHOR: ham_digital_modes:twiddle_q16]

Returns `(cos, sin)` of the twiddle angle `2 pi m n / 256` in Q16.16 from a table of the 256 distinct phases, built once in a
`OnceLock` with the same `cos_q16` and `sin_q16` the per-term evaluation used, so results are bit-identical and the 256-point DFTs
evaluate no trigonometry per term. The index is `(m * n) mod 256` taken with `rem_euclid`, so negative products wrap correctly.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `i32` | Frequency bin, for example -128 to 127. |
| `n` | `i32` | Time index. |

**Returns:** `(cos, sin)` as `(i32, i32)` in Q16.16; for `m*n = 0` it is `(65536, 0)`.

**Errors:** none.

**Special concerns:** the first call builds the table; the `OnceLock` makes that thread safe and later calls are lock free reads.

**Signature:** `fn twiddle_q16(m: i32, n: i32) -> (i32, i32)`

**See also:** test `twiddle_q16_is_the_unit_circle_at_256_phases`.
*(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs`)*

### `phase_dither_q16` [@ANCHOR: ham_digital_modes:phase_dither_q16]

Gives the random phase offset `rho_l(0)` of harmonic `l` (Eq. 141) used when the voiced synthesis starts a harmonic from the unvoiced
state. It maps the noise sample `u_l` in `0..53125` linearly onto the interval from -pi up to just under pi, in Q16.16 radians, as
`u_l * 2pi / 53125 - pi` evaluated as one exact ratio. It is an ordinary bounded angle added in `phi_new_q16`, not an accumulated phase.

| Argument | Type | Meaning |
|---|---|---|
| `noise` | `&NoiseState` | The frame's noise generator. |
| `l` | `u32` | Harmonic number, 1 to 56. |

**Returns:** the angle in Q16.16 radians as `i32`.

**Errors:** panics through `expect` if `l` falls outside the noise window, impossible for 1 to 56.

**Special concerns:** pure, reads the noise state only.

**Signature:** `fn phase_dither_q16(noise: &NoiseState, l: u32) -> i32`

**See also:** test `phase_dither_maps_the_noise_sample_linearly_onto_minus_pi_to_pi`.
*(Reference: `src/ambe/fixed/general/voiced_synthesis.rs`)*

### `dequantize_uniform_q16` [@ANCHOR: ham_digital_modes:dequantize_uniform_q16]

The shared bin-centre formula of Eq. 68 and 71 of the decoder, in Q16.16: `step * (value - 2^(bits-1) + 0.5)`, and 0 for a zero-bit quantiser. It
turns a received quantiser index into the centre of its bin. `reconstruct_gain_vector_q16` and
`reconstruct_higher_order_coefficients_q16` both call it.

| Argument | Type | Meaning |
|---|---|---|
| `quantizer_value` | `u32` | The received index, `0..2^bits`. |
| `bits` | `u8` | Bits allocated to this element; 0 means unquantised. |
| `step_size_q16` | `i32` | Quantiser step, Q16.16. |

**Returns:** the bin centre as Q16.16 `i32`.

**Errors:** none; an index beyond the range just extends the staircase.

**Special concerns:** pure.

**Signature:** `fn dequantize_uniform_q16(quantizer_value: u32, bits: u8, step_size_q16: i32) -> i32`

**See also:** test `dequantize_uniform_returns_the_bin_centre_times_the_step`.
*(Reference: `src/ambe/fixed/tia_102_baba/reconstruct.rs`)*

### `previous_log2_amplitude_q16` [@ANCHOR: ham_digital_modes:previous_log2_amplitude_q16]

Returns `log2(M_tilde_j(-1))` in Q16.16, the previous frame's harmonic amplitude the gain predictor needs, for any `j >= 0`. It applies the
standard's two boundary assumptions (Eq. 56 and 57): for `j = 0` it returns 0, and for `j` above the previous frame's harmonic count
it clamps to the last harmonic. The amplitude table is one-indexed.

| Argument | Type | Meaning |
|---|---|---|
| `previous_m_q16` | `&[i32]` | Previous amplitudes `M_tilde(-1)`, Q16.16, `j = 1..=l_hat_prev` at index `j - 1`. |
| `l_hat_prev` | `u32` | Previous frame's harmonic count; must be at least 1 and at most the slice length. |
| `j` | `u32` | Harmonic index. |

**Returns:** the base-2 logarithm in Q16.16 as `i32`.

**Errors:** an empty table or `l_hat_prev` of 0 with `j > 0` would underflow or index out of range and panic.

**Special concerns:** pure.

**Signature:** `fn previous_log2_amplitude_q16(previous_m_q16: &[i32], l_hat_prev: u32, j: u32) -> i32`

**See also:** test `previous_log2_amplitude_clamps_to_the_last_harmonic_and_is_zero_for_j_zero`.
*(Reference: `src/ambe/fixed/tia_102_baba/prediction.rs`)*

### `partition_into_blocks_q16` [@ANCHOR: ham_digital_modes:partition_into_blocks_q16]

Splits the `L` prediction residuals of a frame into the six blocks of Annex J, whose lengths depend on `L`
(`block_lengths_for_l`). The blocks are consecutive slices of the input, so concatenating them returns the residuals unchanged. The
quantiser then applies a per-block DCT.

| Argument | Type | Meaning |
|---|---|---|
| `residuals_q16` | `&[i32]` | The `L` residuals, Q16.16. |
| `l` | `u32` | Harmonic count, valid 9 to 56. |

**Returns:** `Some([Vec<i32>; 6])`, six owned vectors, or `None`.

**Errors:** `None` when `l` is outside 9 to 56 or `residuals_q16.len() != l`.

**Special concerns:** allocates six vectors per call; pure.

**Signature:** `pub fn partition_into_blocks_q16(residuals_q16: &[i32], l: u32) -> Option<[Vec<i32>; 6]>`

**See also:** test `partition_into_blocks_concatenates_back_to_the_residuals_for_every_l`.
*(Reference: `src/ambe/fixed/tia_102_baba/quantize.rs`)*

## Damaged-frame handling in the float AMBE decoder (slice 5)

### `ErrorPolicy::is_bad` [@ANCHOR: ham_digital_modes_mbe:ErrorPolicy_is_bad]

Decides whether a speech frame counts as damaged, from the corrected-error counts of its two Golay blocks. Under the `Clean` and `Concealing`
policies a frame is bad when the two counts sum to more than 3; under `ChipCompatible` it is bad when the first count is 3 or more and the
second is ignored, reproducing what the real chip does. It is meant to be asked per frame before `bad_frame_action`.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `ErrorPolicy` | `Clean`, `Concealing` (the default) or `ChipCompatible`. |
| `epsilon_c0` | `u32` | Errors corrected in code vector 0. |
| `epsilon_c1` | `u32` | Errors corrected in code vector 1. |

**Returns:** `true` for a damaged frame.

**Errors:** none.

**Special concerns:** pure, `Copy` enum. `ChipCompatible` exists only so conformance tests can compare against the chip; nothing should select it in normal operation.

**Signature:** `pub fn is_bad(self, epsilon_c0: u32, epsilon_c1: u32) -> bool`

**See also:** test `is_bad_follows_each_policys_threshold`.
*(Reference: `src/ambe/float/mbe_synthesis.rs`)*

### `ErrorPolicy::bad_frame_action` [@ANCHOR: ham_digital_modes_mbe:ErrorPolicy_bad_frame_action]

Answers what to do with the n-th damaged frame in a row. For the first three it returns `Repeat` under every policy (use the previous
frame's parameters again); from the fourth on, `Clean` and `Concealing` return `Mute` and `ChipCompatible` returns `Decode`, decoding the
damaged frame as received, which is what the chip does and produces loud bursts, so that policy is a conformance aid only.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `ErrorPolicy` | The policy. |
| `consecutive_bad` | `u32` | 1 for the first damaged frame in a row, 2 for the second, and so on. |

**Returns:** a `BadFrameAction`: `Repeat`, `Mute` or `Decode`.

**Errors:** none; 0 behaves like 1.

**Special concerns:** pure. The caller supplies the running count.

**Signature:** `pub fn bad_frame_action(self, consecutive_bad: u32) -> BadFrameAction`

**See also:** test `bad_frame_action_repeats_three_times_then_mutes_or_decodes`.
*(Reference: `src/ambe/float/mbe_synthesis.rs`)*

### `Descriptor::new` [@ANCHOR: ham_digital_modes_float_conceal:Descriptor_new]

Condenses a decoded frame into what the concealer compares between frames: the fundamental `w0`, the level in dB (RMS of the harmonic amplitudes,
floored at 1e-3), the dB amplitude at sixteen evenly spaced harmonics (each floored at -60 dB) and the voicing of eight evenly
spaced bands. Sampling harmonic `1 + k*L/16` and band `1 + b*L/8` makes frames with different harmonic counts comparable.

| Argument | Type | Meaning |
|---|---|---|
| `w0` | `f64` | Fundamental frequency, radians per sample. |
| `voiced` | `&[bool]` | Voicing per harmonic, one-indexed (index 0 unused). |
| `ml` | `&[f64]` | Harmonic amplitudes, one-indexed; its length is `L + 1`. |

**Returns:** a `Descriptor { w0, level_db, spectrum: [f64; 16], voicing: [bool; 8] }`.

**Errors:** panics if `ml` is empty (the length minus one underflows) or `voiced` is shorter than the indexed band positions; the decoders always pass full-length slices.

**Special concerns:** pure; the descriptors are held by the `Concealer` (previous accepted frame) and discarded afterwards.

**Signature:** `pub fn new(w0: f64, voiced: &[bool], ml: &[f64]) -> Self`

**See also:** test `descriptor_new_reports_level_spectrum_and_band_voicing`; anchor `Concealer_continuity_cost`.
*(Reference: `src/ambe/float/concealment.rs`)*

### `Concealer::continuity_cost` [@ANCHOR: ham_digital_modes_float_conceal:Concealer_continuity_cost]

Computes the cost in nats of accepting a candidate frame given the previous accepted one, so the concealer can choose the most plausible
reading of a damaged frame. The sum has a level term (`|dB difference|` over a scale), a spectral shape term (RMS dB difference over the
16 points over a scale), a cost per voicing band that differs, and, when both frames have any voiced band, a pitch-ratio term
`|ln(w0/w0_prev)|` over a scale. The level, shape and pitch scales are multiplied by `1 + widen_per_repeat * repeats`, so the priors widen
the longer the previous frame has been repeated; the voicing cost does not widen.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&Concealer` | Holds `previous`, `repeats` and the `ConcealParams` scales. |
| `c` | `&Descriptor` | The candidate frame. |

**Returns:** the cost as `f64`, 0 when there is no previous frame.

**Errors:** none.

**Special concerns:** private, called per candidate by `decide`; pure and read only.

**Signature:** `fn continuity_cost(&self, c: &Descriptor) -> f64`

**See also:** test `continuity_cost_adds_level_voicing_and_pitch_terms_and_widens_with_repeats`; anchor `ham_digital_modes:float_concealment_Concealer_decide`.
*(Reference: `src/ambe/float/concealment.rs`)*

## Codec2 3200 fixed-point FFT kernels (fixed_fft, internal stages)

`fft_fixed` and its sparse relatives are built from small kernels, and each is held to the same rule: it must give the integers the textbook
`i128` loop gives. Three transform families share them: the dense stage loop (the `block` family), the sparse-prefix transform for an input whose
only nonzero samples are the first few (the pitch estimator's 64-sample window and the LPC spectrum), and the sparse harmonic inverse
(`SparseInverse`) that skips empty blocks. The generic parameters `FWD` (true: forward, twiddle `(wr, wif)`; false: inverse, `wi = -wif`) and `MODE`
(`MODE_CHECKED`, `MODE_I64`, `MODE_I32`, `MODE_I64_NATIVE`) select the direction and the arithmetic width; `mode_for_sum` (documented with the
earlier fixed_fft entries) picks the mode from the sum of input magnitudes. All of these functions are private (or `pub(crate)`), have no side effects
beyond the buffers passed in, take no locks and never block; none can fail at run time (index errors would be a bug and panic).

### `bf` [@ANCHOR: fixed_fft:bf]

The generic radix-2 decimation-in-time butterfly, used by every stage that has no cheaper form. It reads the pair `(a, b)` and the twiddle `w`, computes
`v = w * b` rounded to Q23, and writes `a' = a + v`, `b' = a - v`. Called from `block` for every position other than the two exactly
representable twiddles. The arithmetic is chosen at compile time by `MODE`: 32-bit scaled or plain, 64-bit native, 64-bit split into halves, or the
exact checked `twiddle_mul` (which switches to `i128` for very large values).

| Argument | Type | Meaning |
|---|---|---|
| `ar`, `ai` | `&mut i64` | Real and imaginary parts of the lower element, Q23, updated in place |
| `br`, `bi` | `&mut i64` | Real and imaginary parts of the upper element, Q23, updated in place |
| `(wr, wif)` | `(i32, i32)` | Table twiddle, `(cos, -sin)` for the forward transform, Q23; the inverse applies `wi = -wif` |

**Returns:** nothing; the four values are overwritten.
**Errors:** none; the checked mode is exact, and the 32-bit and 64-bit modes are only selected when the value-sum bounds hold.
**Special concerns:** `#[inline(always)]`; `MODE_I32` requires every value under 2^29 and a twiddle strictly under 2^23 in magnitude (see `twiddles_below_unity`).
**Signature:** `fn bf<const FWD: bool, const MODE: u8>(ar, ai, br, bi: &mut i64, (wr, wif): (i32, i32))`
**See also:** test `butterfly_kernels_agree_bit_for_bit`; `bf_one`, `bf_quarter`, `i32_twiddle_scaled`, `fixed_fft:twiddle_mul`.

### `bf_one` [@ANCHOR: fixed_fft:bf_one]

The butterfly for the twiddle `1` (position `j == 0` of every block), where the multiply is the identity (`(x * 2^23 + 2^22) >> 23 == x`) and only an add
and a subtract remain. `block` calls it for the first pair of each block, so it is the cheapest butterfly and runs once per block per stage. It
is bit-identical to `bf` with the table entry for `j == 0`, in every mode and both directions, which is what makes skipping the multiply safe.

| Argument | Type | Meaning |
|---|---|---|
| `ar`, `ai` | `&mut i64` | Lower element, Q23, replaced by `a + b` |
| `br`, `bi` | `&mut i64` | Upper element, Q23, replaced by `a - b` |

**Returns:** nothing.
**Errors:** none; plain `i64` addition, so the caller must keep values under the bound for its mode (every mode's bound is far below `i64::MAX`).
**Special concerns:** `#[inline(always)]`, no side effects.
**Signature:** `fn bf_one(ar, ai, br, bi: &mut i64)`
**See also:** test `optimized_fft_is_bit_identical_to_the_textbook_i128_loop`; `bf`, `bf_quarter`, `block`.

### `bf_quarter` [@ANCHOR: fixed_fft:bf_quarter]

The butterfly for the quarter-turn twiddle at `j == half/2`, whose table entry is exactly `(0, -2^23)`, that is `-j` for the forward transform and `+j` for the
inverse. Multiplying by `-j` only swaps the components and flips a sign, so no multiply or rounding is needed. `block` and `block_inv_i64` call it
for the middle pair of each block; the result is bit-identical to `bf` at that position.

| Argument | Type | Meaning |
|---|---|---|
| `FWD` | const `bool` | Forward (multiply by `-j`) or inverse (`+j`) |
| `ar`, `ai` | `&mut i64` | Lower element, Q23, replaced by `a + w b` |
| `br`, `bi` | `&mut i64` | Upper element, Q23, replaced by `a - w b` |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** `#[inline(always)]`; exact for any `i64` values whose sums do not overflow.
**Signature:** `fn bf_quarter<const FWD: bool>(ar, ai, br, bi: &mut i64)`
**See also:** test `optimized_fft_is_bit_identical_to_the_textbook_i128_loop`; `bf_one`, `block`.

### `block` [@ANCHOR: fixed_fft:block]

Merges one block of one stage: the `len` positions starting at `base` hold two finished half-size transforms (`lo` is the first `len/2`, `hi` the second)
and are combined in place into one transform of size `len`. It does `bf_one` at `j == 0`, `bf_quarter` at `j == half/2` and `bf` for every other `j` with
the twiddle `tw[j * step]`. When `hi_zero` is set (sparse input) the upper half is all zero, every butterfly is `(a, a)`, and the block is a plain copy.
Called by `stage_dense` (test oracle), `stage_prefix` and `stage_sparse`.

| Argument | Type | Meaning |
|---|---|---|
| `re`, `im` | `&mut [i64]` | Whole transform arrays, Q23, in bit-reversed order before the stage |
| `tw` | `&[(i32, i32)]` | Twiddle table `(wr, wif)` for the transform size |
| `base` | `usize` | First position of this block |
| `len` | `usize` | Block size, a power of two; a literal in the hot monomorphisations, a run-time value in the compact loops |
| `step` | `usize` | `N / len`, the stride into `tw` |
| `hi_zero` | `bool` | Upper half is all zero; copy the lower half up instead of multiplying |

**Returns:** nothing.
**Errors:** panics on out-of-range slice indices only (a caller bug).
**Special concerns:** `#[inline(always)]`; writes only `base..base + len`.
**Signature:** `fn block<const FWD: bool, const MODE: u8>(re, im, tw, base, len, step, hi_zero)`
**See also:** test `optimized_fft_is_bit_identical_to_the_textbook_i128_loop`; `bf`, `bf_one`, `bf_quarter`, `block_inv_i64`.

### `block_inv_i64` [@ANCHOR: fixed_fft:block_inv_i64]

`block` specialised for the inverse direction in the 64-bit mode (every value under 2^39), with no sign corrections in the multiplies. For the
inverse table `wi = -wif > 0` everywhere, `wr > 0` for `j < half/2` and `wr < 0` above it, so each product uses unsigned magnitudes. For `j < q` the product
is `(A, B)` from `twiddle_sums`; for `j > q` it is `(-B', -A')` for the sign-flipped twiddle with the data components swapped. With `SYM` true the table must be
`quadrant_symmetric` and the upper quadrant reuses first-quadrant entries by an exact quarter-turn rotation of the data. Bit-identical to the generic butterfly.

| Argument | Type | Meaning |
|---|---|---|
| `SYM` | const `bool` | The table is quadrant-symmetric (the decoder's, not the pitch estimator's) |
| `re`, `im` | `&mut [i64]` | Transform arrays, Q23, updated in the block |
| `tw` | `&[(i32, i32)]` | Twiddle table |
| `base`, `len`, `step` | `usize` | Block start, block size, table stride (`N / len`) |

**Returns:** nothing.
**Errors:** none at run time; a table without the stated signs or symmetry would give wrong integers, which the const checks `quadrant_symmetric` and `twiddles_below_unity` guard.
**Special concerns:** only valid when the value sum is under `I64_SUM_LIMIT` (2^39); callers select it with `mode_for_sum`. `#[inline(always)]`.
**Signature:** `fn block_inv_i64<const SYM: bool>(re, im, tw, base, len, step)`
**See also:** test `optimized_fft_is_bit_identical_to_the_textbook_i128_loop`; `block_inv_i64_pruned`, `quadrant_symmetric`, `fixed_fft:twiddle_sums`.

### `block_inv_i64_pruned` [@ANCHOR: fixed_fft:block_inv_i64_pruned]

`block_inv_i64` for a non-symmetric table, writing only the outputs that the const `PRUNE` keeps. The pitch estimator reads only bins 0 to 128 of a
512-point transform, so its last stage (`PRUNE_LAST`) and the stage before it (`PRUNE_PENULTIMATE`) skip everything those bins do not depend on. Every
written value uses exactly the arithmetic of `block_inv_i64`, so it is bit-identical; the other positions are left unspecified for the caller.

| Argument | Type | Meaning |
|---|---|---|
| `PRUNE` | const `u8` | `PRUNE_LAST` (1) or `PRUNE_PENULTIMATE` (2): which stage's output is pruned |
| `MODE` | const `u8` | `MODE_I64` or `MODE_I64_NATIVE` |
| `re`, `im`, `tw`, `base`, `len`, `step` | as `block_inv_i64` | Same meaning |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** called by `stage_prefix` only when `PRUNE`, the inverse direction, a 64-bit mode and `len` equal to `N` or `N / 2`. Positions it skips hold stale data.
**Signature:** `fn block_inv_i64_pruned<const PRUNE: u8, const MODE: u8>(re, im, tw, base, len, step)`
**See also:** test `sparse_prefix_fft_is_bit_identical_to_the_dense_reference`; `pitch_fft_512`, `stage_prefix`.

### `stage_prefix` [@ANCHOR: fixed_fft:stage_prefix]

One stage of the sparse-prefix transform, for natural-order input nonzero only in `0..nz`. The block of `len` positions holding natural indices `r, r + step, ...`
(`step = N / len`) sits at bit-reversed position `bitrev[r]`; it is all zero when `r >= nz`, and its upper half is zero when `r + step >= nz`. Zero blocks are never
touched (their parent sees a zero upper half), and a block with a zero upper half is a run of one repeated input already written by `scatter_prefix`, so it is skipped
too. Only blocks with `r + step < nz` get a butterfly block, so the arrays need no clearing. It picks `block_inv_i64_pruned`, `block_inv_i64` or `block`.

| Argument | Type | Meaning |
|---|---|---|
| `N`, `FWD`, `MODE`, `PRUNE` | const | Size, direction, arithmetic mode, and whether to prune for the pitch estimator |
| `re`, `im` | `&mut [i64; N]` | Transform arrays |
| `tw`, `bitrev` | slices | Twiddle table and bit-reversal table |
| `len` | `usize` | Block size of this stage |
| `nz` | `usize` | Count of nonzero leading inputs |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** `#[inline(always)]`; arrays must have been filled by `scatter_prefix` first.
**Signature:** `fn stage_prefix<const N, const FWD, const MODE, const PRUNE>(re, im, tw, bitrev, len, nz)`
**See also:** test `sparse_prefix_fft_is_bit_identical_to_the_dense_reference`; `stages_prefix_hot`, `stages_prefix_loop`, `prefix_fill_len`.

### `stages_prefix_hot` [@ANCHOR: fixed_fft:stages_prefix_hot]

Runs every stage of the sparse-prefix transform with the stage length as a literal, by expanding `stage_prefix` for lengths 2, 4, ... 1024 (those not
above `N`). Literals make every address, trip count and twiddle stride a compile-time constant, which is the fast path used by the production LPC-spectrum
and pitch calls (via `prefix_forward_i32_hot` and `pitch_prefix_i64_hot`). It is larger in code than `stages_prefix_loop`.

| Argument | Type | Meaning |
|---|---|---|
| `N`, `FWD`, `MODE`, `PRUNE` | const | As `stage_prefix` |
| `re`, `im` | `&mut [i64; N]` | Scattered input, replaced by the transform |
| `tw`, `bitrev` | slices | Tables |
| `nz` | `usize` | Nonzero leading inputs |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** `#[inline(always)]`.
**Signature:** `fn stages_prefix_hot<const N, const FWD, const MODE, const PRUNE>(re, im, tw, bitrev, nz)`
**See also:** test `pitch_transform_matches_the_dense_reference_in_both_64_bit_kernels`; `stages_prefix_loop`, `stage_prefix`.

### `stages_prefix_loop` [@ANCHOR: fixed_fft:stages_prefix_loop]

The compact fallback of `stages_prefix_hot`: one copy of the stage loop per size, direction and mode, with the stage length a run-time variable that doubles
from 2 to `N`. It is `#[inline(never)]` and used for the cold modes (inputs too large for the fast paths) by `prefix_general`, where code size matters
more than speed. It never prunes (`PRUNE` is false) and computes the same integers as the hot form.

| Argument | Type | Meaning |
|---|---|---|
| `N`, `FWD`, `MODE` | const | Size, direction, arithmetic mode |
| `re`, `im` | `&mut [i64; N]` | Scattered input, replaced by the transform |
| `tw`, `bitrev` | slices | Tables |
| `nz` | `usize` | Nonzero leading inputs |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** no side effects beyond the arrays.
**Signature:** `fn stages_prefix_loop<const N, const FWD, const MODE>(re, im, tw, bitrev, nz)`
**See also:** test `sparse_prefix_fft_is_bit_identical_to_the_dense_reference`; `prefix_general`.

### `prefix_general` [@ANCHOR: fixed_fft:prefix_general]

The compact, any-mode form of the sparse-prefix transform for an arbitrary twiddle table (the decoder's, or the pitch estimator's). It sums the input
magnitudes, picks the mode with `mode_for_sum`, scatters each input with `scatter_prefix` and runs `stages_prefix_loop`. It is the fallback of
`pitch_fft_512_kernel` (and, through `fft_fixed_sparse_prefix`, of `fft_fixed_sparse_prefix_forward`) when the input is too large for the fast kernels.

| Argument | Type | Meaning |
|---|---|---|
| `input` | `&[i64]` | Real samples in natural order, Q23; the rest of the transform input is zero |
| `re`, `im` | `&mut [i64; N]` | Output arrays; need no clearing |
| `forward` | `bool` | Transform direction |
| `tw`, `bitrev` | slices | Twiddle and bit-reversal tables |

**Returns:** nothing; the arrays hold the full transform.
**Errors:** a debug assertion requires `1 <= nz <= N`; otherwise none, it is exact for any input within `i64`.
**Special concerns:** deterministic, no allocation.
**Signature:** `fn prefix_general<const N: usize>(input: &[i64], re, im, forward: bool, tw, bitrev)`
**See also:** test `sparse_prefix_fft_is_bit_identical_to_the_dense_reference`; `fft_fixed_sparse_prefix`.

### `scatter_prefix` [@ANCHOR: fixed_fft:scatter_prefix]

Places the real input value `v` for natural index `k` at its bit-reversed position `bitrev[k]`, and fills the `fill` consecutive positions of the block that
stay constant while its upper half is zero (see `prefix_fill_len`) with `v`; the imaginary part is set to zero there. This single write replaces
copying the value up stage by stage and means no array needs clearing beforehand. Called once per nonzero input by the sparse-prefix transforms.

| Argument | Type | Meaning |
|---|---|---|
| `re`, `im` | `&mut [i64; N]` | Output arrays |
| `bitrev` | `&[u16]` | Bit-reversal table |
| `k` | `usize` | Natural index of the input, `k < nz` |
| `fill` | `usize` | Run length from `prefix_fill_len` |
| `v` | `i64` | Real input value, Q23 |

**Returns:** nothing.
**Errors:** panics if `bitrev[k] + fill` exceeds `N` (a caller bug).
**Special concerns:** `#[inline(always)]`.
**Signature:** `fn scatter_prefix<const N: usize>(re, im, bitrev, k, fill, v)`
**See also:** test `sparse_prefix_fft_is_bit_identical_to_the_dense_reference`; `scatter_prefix_short`, `prefix_fill_len`.

### `scatter_prefix_short` [@ANCHOR: fixed_fft:scatter_prefix_short]

`scatter_prefix` for the pitch estimator, whose runs are all short (8 entries): the imaginary zeros are stored inline instead of with `fill(0)`.
A `fill(0)` compiles to a call to `memset`, whose call overhead dominates on a core without an optimised library (the 32-bit targets). The zero is opaque to the
optimiser on purpose, because a loop that stores a literal zero is turned back into that call. The result is identical to `scatter_prefix`.

| Argument | Type | Meaning |
|---|---|---|
| `re`, `im` | `&mut [i64; N]` | Output arrays |
| `bitrev` | `&[u16]` | Bit-reversal table |
| `k`, `fill`, `v` | as `scatter_prefix` | Natural index, run length (short), value |

**Returns:** nothing.
**Errors:** panics on out-of-range index (caller bug).
**Special concerns:** `#[inline(always)]`; called by `pitch_fft_512_kernel`.
**Signature:** `fn scatter_prefix_short<const N: usize>(re, im, bitrev, k, fill, v)`
**See also:** test `pitch_transform_matches_the_dense_reference_in_both_64_bit_kernels`; `scatter_prefix`.

### `prefix_fill_len` [@ANCHOR: fixed_fft:prefix_fill_len]

A `const fn` giving the length of the run of identical values that block `r` holds while its upper half is still all zero: the block then contains just input `r`
repeated, so it can be written directly. The block exists (`r < N / len`) and has a zero upper half (`r + N / len >= nz`) exactly while `len <= N / max(nz - r, r + 1)`;
this returns the largest such power of two. It builds the per-input fill table `FillLens` evaluated at compile time for each `(N, NZ)` pair.

| Argument | Type | Meaning |
|---|---|---|
| `n` | `usize` | Transform size |
| `nz` | `usize` | Count of nonzero leading inputs |
| `r` | `usize` | Natural index of the block (residue) |

**Returns:** `usize`, the run length (a power of two, at least 1).
**Errors:** none; pure and const-evaluable.
**Special concerns:** compile-time only in production.
**Signature:** `const fn prefix_fill_len(n: usize, nz: usize, r: usize) -> usize`
**See also:** test `const_table_checks_accept_the_real_tables_and_reject_broken_ones`; `scatter_prefix`.

### `fft_fixed_sparse_prefix_forward` [@ANCHOR: fixed_fft:fft_fixed_sparse_prefix_forward]

`fft_fixed_sparse_prefix` with the nonzero count `NZ` a compile-time constant and the forward direction: the production LPC-spectrum call. It sums the
magnitudes of the input; when the sum is under `I32_SUM_LIMIT` (always, for real LPC coefficients) it scatters with the constant `FillLens` table and runs the whole
transform in one fully specialised instance (`prefix_forward_i32_hot`), where every block position, copy decision and twiddle stride is a constant.
Otherwise it falls back to `fft_fixed_sparse_prefix`.

| Argument | Type | Meaning |
|---|---|---|
| `input` | `&[i64; NZ]` | The `NZ` leading real inputs, Q23 |
| `re`, `im` | `&mut [i64; N]` | Output arrays, fully overwritten |

**Returns:** nothing; same integers as `fft_fixed` on the zero-padded input.
**Errors:** none.
**Special concerns:** `pub(crate)`; needs `Size<N>: FftTables`; no locks, no allocation.
**Signature:** `pub(crate) fn fft_fixed_sparse_prefix_forward<const N: usize, const NZ: usize>(input: &[i64; NZ], re, im)`
**See also:** test `sparse_prefix_fft_is_bit_identical_to_the_dense_reference`; `fft_fixed_sparse_prefix`.

### `pitch_fft_512_kernel` [@ANCHOR: fixed_fft:pitch_fft_512_kernel]

`pitch_fft_512` with the "values under 2^39" kernel named by the const `K` (`MODE_I64` or `MODE_I64_NATIVE`), so the tests can run both on the same inputs and
require identical output. It sums the input, and if under `I64_SUM_LIMIT` scatters with `scatter_prefix_short` and runs `pitch_prefix_i64_hot` (pruned
inverse stages with the pitch estimator's own twiddle table); otherwise it uses `prefix_general`. Only bins 0 to 128 are meaningful afterwards.

| Argument | Type | Meaning |
|---|---|---|
| `NZ` | const `usize` | Number of nonzero leading inputs (the estimator's window) |
| `K` | const `u8` | Kernel: `MODE_I64` or `MODE_I64_NATIVE` |
| `input` | `&[i64; NZ]` | Windowed real samples, Q23 |
| `re`, `im` | `&mut [i64; FFT_ENC]` | 512-point output arrays |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** private; production code calls `pitch_fft_512`, which fixes `K = I64_KERNEL`.
**Signature:** `fn pitch_fft_512_kernel<const NZ: usize, const K: u8>(input, re, im)`
**See also:** test `pitch_transform_matches_the_dense_reference_in_both_64_bit_kernels`; `stages_prefix_hot`.

### `SparseInverse_put` [@ANCHOR: fixed_fft:SparseInverse_put]

Method `SparseInverse::put`: sets spectrum bin `b` (`1 <= b < N / 2`) and its conjugate mirror `N - b`. The value goes straight to the bit-reversed positions
`bitrev[b]` and `bitrev[N - b]` (the mirror with its imaginary part negated), the occupancy flags for both bins are set, and a running upper bound of the
transform's value sum grows by `2 * (|re| + |im|)`. A later `put` at the same bin overwrites the earlier one, as the dense fill did.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut SparseInverse<N>` | The builder; owns the flag words and the sum bound |
| `b` | `usize` | Bin index, `1 <= b < N / 2` |
| `v` | `ComplexQ23` | Bin value, Q23 |

**Returns:** nothing.
**Errors:** panics on `b` out of range (caller bug).
**Special concerns:** `#[inline(always)]`; the overwritten bin still counts in the sum bound, which only loosens it.
**Signature:** `pub(crate) fn put(&mut self, b: usize, v: ComplexQ23)`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `SparseInverse_run_kernel`.

### `SparseInverse_run_kernel` [@ANCHOR: fixed_fft:SparseInverse_run_kernel]

Method `SparseInverse::run_kernel`, the body of `run`: consumes the builder, runs `fold_flags`, clears either half of the buffers that the flags show to be entirely zero
(so the ordinary last stage applies), then, when the sum bound is under `I64_SUM_LIMIT`, runs `sparse_inverse_hot` (`stages_sparse_hot` plus `final_stage_real_hot`) with
kernel `K`; otherwise it runs `stages_sparse_loop` and `final_stage_real_checked` in the exact mode. Afterwards only the real parts at `0..=NS` and `N - NS + 1 .. N` are valid.

| Argument | Type | Meaning |
|---|---|---|
| `NS` | const `usize` | Highest output index used, `NS < N / 4` |
| `K` | const `u8` | `MODE_I64` or `MODE_I64_NATIVE` (tests run both) |
| `self` | `SparseInverse<N>` | Consumed |

**Returns:** nothing; results are in `re` of the borrowed arrays.
**Errors:** none.
**Special concerns:** all other positions of `re`, and all of `im`, are undefined afterwards.
**Signature:** `fn run_kernel<const NS: usize, const K: u8>(mut self)`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `SparseInverse_put`, `fold_flags`.

### `fold_flags` [@ANCHOR: fixed_fft:fold_flags]

Fills the occupancy-flag levels above the input level for the sparse inverse. The block of stage `s` (`step = N >> s` residues) with residue `r` is nonzero
when either of its halves (residues `r` and `r + step` of the previous level) is; level `s` starts at bit `2N - 2 * step`. Large levels (`step >= 32`) are
folded a word at a time with OR, small ones bit by bit. `SparseInverse::run_kernel` calls it once before the stages.

| Argument | Type | Meaning |
|---|---|---|
| `N` | const `usize` | Transform size |
| `flags` | `&mut [u32; FLAG_WORDS]` | Bit set; input level already filled by `put`, higher levels filled here (`FLAG_WORDS = 2 * 1024 / 32`) |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** private; deterministic.
**Signature:** `fn fold_flags<const N: usize>(flags: &mut [u32; FLAG_WORDS])`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `stage_sparse`.

### `stage_sparse` [@ANCHOR: fixed_fft:stage_sparse]

One stage of the sparse inverse: only blocks whose flag is set are touched. A block whose upper half is all zero is a copy of its lower half (`block` with `hi_zero`);
one whose lower half is all zero gets that half cleared first and then takes the ordinary butterfly (rare, so it has no kernel of its own). Other blocks use
`block_inv_i64::<true>` in `MODE_I64`, otherwise `block`. It iterates the first `step` residues in bit-reversed order.

| Argument | Type | Meaning |
|---|---|---|
| `N`, `MODE` | const | Size and arithmetic mode |
| `re`, `im` | `&mut [i64; N]` | Transform arrays |
| `tw`, `bitrev` | slices | Tables |
| `flags` | `&[u32; FLAG_WORDS]` | Occupancy flags from `fold_flags` |
| `len` | `usize` | Block size of this stage |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** `#[inline(always)]`; never reads or writes a block whose flag is clear.
**Signature:** `fn stage_sparse<const N: usize, const MODE: u8>(re, im, tw, bitrev, flags, len)`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `stages_sparse_hot`.

### `stages_sparse_hot` [@ANCHOR: fixed_fft:stages_sparse_hot]

Runs every stage of the sparse inverse except the last (lengths 2 to 512, those under `N`) with the stage length a literal, by expanding `stage_sparse`.
The last stage is done separately by `final_stage_real`. Used by `sparse_inverse_hot` on the fast path where the value sum is under 2^39.

| Argument | Type | Meaning |
|---|---|---|
| `N`, `MODE` | const | Size and mode |
| `re`, `im` | `&mut [i64; N]` | Arrays holding the scattered bins |
| `tw`, `bitrev`, `flags` | slices | Tables and flags |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** `#[inline(always)]`.
**Signature:** `fn stages_sparse_hot<const N: usize, const MODE: u8>(re, im, tw, bitrev, flags)`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `stages_sparse_loop`.

### `stages_sparse_loop` [@ANCHOR: fixed_fft:stages_sparse_loop]

The compact form of `stages_sparse_hot`: a run-time loop doubling the stage length from 2 while it is under `N`, calling `stage_sparse`. It is
`#[inline(never)]` and is used in `MODE_CHECKED` (exact arithmetic) when the value-sum bound is at or above 2^39, a case not expected from real speech (unknown whether any test exercises it).

| Argument | Type | Meaning |
|---|---|---|
| `N`, `MODE` | const | Size and mode |
| `re`, `im` | `&mut [i64; N]` | Arrays |
| `tw`, `bitrev`, `flags` | slices | Tables and flags |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** no side effects beyond the arrays.
**Signature:** `fn stages_sparse_loop<const N: usize, const MODE: u8>(re, im, tw, bitrev, flags)`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `stages_sparse_hot`.

### `final_stage_real` [@ANCHOR: fixed_fft:final_stage_real]

The last stage of the inverse transform, computing only what the synthesis reads: the real parts of outputs `0..=NS` (lower half) and of
`N - NS + 1 .. N` (the upper half, indices `N/2 - NS + 1 .. N/2` of the upper block). Nothing else is written, and imaginary parts are never produced,
which saves about half the multiplies of the final stage. Uses `twiddle_real_inverse` for the real part of each product. Requires `NS < N / 4`.

| Argument | Type | Meaning |
|---|---|---|
| `N`, `NS`, `MODE` | const | Size, highest needed output, arithmetic mode |
| `re`, `im` | `&mut [i64; N]` | Arrays after the previous stage; `re` receives the outputs |
| `tw` | `&[(i32, i32)]` | Twiddle table |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** `#[inline(always)]`; all other entries are left as they were.
**Signature:** `fn final_stage_real<const N: usize, const NS: usize, const MODE: u8>(re, im, tw)`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `twiddle_real_inverse`.

### `twiddle_real_inverse` [@ANCHOR: fixed_fft:twiddle_real_inverse]

Computes the real part of `(wr + j wi) * (br + j bi)` rounded back to Q23, for the inverse direction (`wi = -wif`), in the given mode. It exists for
`final_stage_real`, which needs only real outputs, so the imaginary product is never formed. The result equals the real part from the generic `bf`
in the same mode.

| Argument | Type | Meaning |
|---|---|---|
| `MODE` | const `u8` | Arithmetic mode |
| `(wr, wif)` | `(i32, i32)` | Table twiddle, Q23 |
| `br`, `bi` | `i64` | Complex operand, Q23 |

**Returns:** `i64`, the rounded real part, Q23.
**Errors:** none.
**Special concerns:** `#[inline(always)]`; pure.
**Signature:** `fn twiddle_real_inverse<const MODE: u8>((wr, wif): (i32, i32), br: i64, bi: i64) -> i64`
**See also:** test `sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs`; `final_stage_real`.

### `i32_twiddle_scaled` [@ANCHOR: fixed_fft:i32_twiddle_scaled]

The 32-bit-core twiddle multiply of the `MODE_I32` butterfly. It gives the same integers as `i32_twiddle_plain`, but pre-scales the twiddle by `2^8` and the operand by 2, so
the rounded Q23 result is the high word of a 64-bit sum plus `2^31`: a signed 32x32 to 64 multiply (`mul` and `mulh`) with no cross-word shift. `bf` calls it when the
target has no native 64-bit multiplier.

| Argument | Type | Meaning |
|---|---|---|
| `wr`, `wi` | `i32` | Twiddle, strictly under `2^23` in magnitude, Q23 |
| `yr`, `yi` | `i32` | Operand, magnitude under `2^29`, Q23 |

**Returns:** `(i32, i32)`, real and imaginary parts of `w * y`, rounded to Q23.
**Errors:** none; the bounds are guaranteed by `mode_for_sum` and `twiddles_below_unity`.
**Special concerns:** `#[inline(always)]`.
**Signature:** `fn i32_twiddle_scaled(wr: i32, wi: i32, yr: i32, yi: i32) -> (i32, i32)`
**See also:** test `i32_twiddle_kernels_agree_bit_for_bit`; `bf`, `twiddles_below_unity`.

### `narrow_twiddles` [@ANCHOR: fixed_fft:narrow_twiddles]

A `const fn` that narrows a twiddle table from `i64` pairs to `i32` pairs at compile time. Every entry is at most `2^23` in magnitude, so the narrowing
is exact; the `i64` source tables stay as the checked-in, regenerated-and-diffed reference data, and the run-time table is half the size. A value that does not fit
fails the compile.

| Argument | Type | Meaning |
|---|---|---|
| `M` | const `usize` | Table length |
| `src` | `&[(i64, i64); M]` | Reference table `(wr, wif)`, Q23 |

**Returns:** `[(i32, i32); M]`.
**Errors:** a compile-time assertion failure if any entry does not fit `i32`.
**Special concerns:** const-evaluated; no run-time cost.
**Signature:** `const fn narrow_twiddles<const M: usize>(src: &[(i64, i64); M]) -> [(i32, i32); M]`
**See also:** test `const_table_checks_accept_the_real_tables_and_reject_broken_ones`; `twiddles_below_unity`, `quadrant_symmetric`.

### `quadrant_symmetric` [@ANCHOR: fixed_fft:quadrant_symmetric]

A `const fn` predicate: is the table's second quadrant its first rotated by a quarter turn, `w[N/4 + m] == -i * w[m]`, i.e. `(wr, wif)` becomes `(wif, -wr)`, exactly as
integers? It is true of the decoder's tables and not of the pitch estimator's. `block_inv_i64::<true>` relies on it to multiply by a first-quadrant twiddle after
an exact quarter-turn rotation of the data, which is cheaper. Evaluated at compile time to vet the tables.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&[(i32, i32); M]` | Twiddle table `(wr, wif)`, Q23, half the transform size long |

**Returns:** `bool`, true when the symmetry holds for every `m`.
**Errors:** none.
**Special concerns:** const-evaluated.
**Signature:** `const fn quadrant_symmetric<const M: usize>(t: &[(i32, i32); M]) -> bool`
**See also:** test `const_table_checks_accept_the_real_tables_and_reject_broken_ones`; `block_inv_i64`.

### `twiddles_below_unity` [@ANCHOR: fixed_fft:twiddles_below_unity]

A `const fn` predicate: every entry other than the first (exactly `1`) and the quarter turn (exactly `-j`), which the butterflies never multiply by, is strictly under
`2^23` in magnitude in both components. That is what lets the 32-bit butterfly pre-scale a twiddle by `2^8` inside an `i32` (see `i32_twiddle_scaled`). Used in compile-time
assertions on the real tables.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&[(i32, i32); M]` | Twiddle table, Q23 |

**Returns:** `bool`, true when the bound holds.
**Errors:** none.
**Special concerns:** const-evaluated.
**Signature:** `const fn twiddles_below_unity<const M: usize>(t: &[(i32, i32); M]) -> bool`
**See also:** test `const_table_checks_accept_the_real_tables_and_reject_broken_ones`; `narrow_twiddles`, `bf`.

*(Reference: `src/codec2_3200/fixed_fft.rs`)*

## AMBE codec helpers anchored in slice 5 (quantizers, classifiers, interleave, FEC encoders)

Each function here already had an exact-value or float-parity test; the tests cite them. These are the chip-compatible FEC encoders, the D-STAR and AMBE+2
dequantizers and frame classifiers, the pitch quantizers, and the D-STAR wire interleave and frame builder. All are plain synchronous functions: no
I/O, no locks, no global state, no transmitter keying. They sit on the vocoder path only (frames in memory), so no PTT, band or transmit-gate logic applies.

### `g3_encode` [@ANCHOR: ham_digital_modes:dvsi_p25fec_fec_g3_encode]

Encodes 8 data bits into the 23-bit `g3` codeword the DVSI chip emits for its third channel block. It is not a standard systematic Golay construction: the codeword is
the XOR of the rows of `G3_GENERATOR` (a rank-8 basis derived from about 3500 captured chip frames) selected by the data bits, most significant bit first. The test
side uses it to build chip-style frames and to check that `g3_decode` round-trips all 256 codewords.

| Argument | Type | Meaning |
|---|---|---|
| `data` | `u8` | The 8 data bits, MSB first (bit 7 selects row 0) |

**Returns:** `u32`, the 23-bit codeword in the low bits; zero data gives zero.
**Errors:** none; every `u8` is valid.
**Special concerns:** pure, constant time. What parameter the 8-bit subspace represents is documented in the source as still open.
**Signature:** `pub fn g3_encode(data: u8) -> u32`
**See also:** tests `g3_encode_of_zero_is_zero`, `g3_decode_round_trips_every_one_of_its_256_real_codewords_with_zero_distance`; `fec_g3_decode`.

### `hamming_encode_chip` [@ANCHOR: ham_digital_modes:dvsi_p25fec_fec_hamming_encode_chip]

Encodes 11 data bits into the chip's real 15-bit systematic Hamming codeword: data in the high 11 bits, 4 parity bits in the low bits. Parity is the XOR of the
`HAMMING_PARITY_CHIP` rows for each set data bit (bit 10 selects row 0). The same abstract [15,11,3] code as the one in `fec.rs`, but with the chip's own column labelling.
`hamming_decode_chip` inverts it by brute-force minimum distance.

| Argument | Type | Meaning |
|---|---|---|
| `data` | `u16` | Data bits; only the low 11 are used (masked with `0x07FF`) |

**Returns:** `u16`, the 15-bit codeword.
**Errors:** none; high bits of `data` are ignored.
**Special concerns:** pure, constant time.
**Signature:** `pub fn hamming_encode_chip(data: u16) -> u16`
**See also:** tests `hamming_encode_chip_of_zero_is_zero`, `hamming_chip_weight_distribution_matches_the_known_enumerator`; `hamming_decode_chip`.

### `dequantize` (fixed AMBE+2) [@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_decode_dequantize]

The fixed-point AMBE+2 dequantizer: turns the raw `b0..b8` indices of one frame into speech parameters, and mirrors the float `dequantize` over representative sequences. It branches on
`classify_b0`: erasure returns `Erasure`; the tone sub-codes return `Tone` carrying the raw parameters; silence returns `Silence` with `L = 14` and resets the decoder state;
otherwise `L` and `w0` come from `L_TABLE` and the Q16/Q32 `W0_TABLE` (a table of f0, multiplied by 2 pi) and `dequantize_speech` applies the differential state.

| Argument | Type | Meaning |
|---|---|---|
| `raw` | `&RawParameters` | The frame's extracted indices `b0` to `b8` |
| `state` | `&mut MbeDecoderState` | Previous-frame decoder state (`l`, `gamma_q16`, log2 magnitudes); updated in place |

**Returns:** `DequantizedFrame`: `Erasure`, `Tone { raw }`, `Silence { l, w0_q16 }` or `Speech(parameters)`.
**Errors:** none returned. `b0` of 0 to 119 indexes the tables; other values never reach the table lookups.
**Special concerns:** the state advances only on speech and silence; erasures and tones leave it untouched, so the caller decides concealment.
**Signature:** `pub fn dequantize(raw: &RawParameters, state: &mut MbeDecoderState) -> DequantizedFrame`
**See also:** test `fixed_dequantize_matches_float_across_representative_sequences`; `float_ambe_plus_2_decode_classify_b0`.

### `dequantize` (fixed D-STAR) [@ANCHOR: ham_digital_modes:fixed_dstar_decode_dequantize]

The fixed-point D-STAR dequantizer. It extracts the raw indices from the 49-bit parameter word `d`; a tone frame (`b0 & 0x7E == 0x7E`) returns `Tone` from `decode_tone`; otherwise `L` and
`w0` come from the pitch tables by `b0` (clamped to the table length) and `dequantize_speech` reconstructs the speech parameters, updating the state. It gives the same parameters as the float
D-STAR `dequantize` over representative frame sequences.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `u64` | The 49-bit parameter layout (see `extract_raw_parameters`) |
| `state` | `&mut MbeDecoderState` | Previous-frame state, updated on speech frames |

**Returns:** `DequantizedFrame::Speech(parameters)` or `DequantizedFrame::Tone(payload)`.
**Errors:** none; out-of-range `b0` is clamped to the last table entry.
**Special concerns:** pure apart from the state update; no erasure variant (D-STAR damage is handled elsewhere).
**Signature:** `pub fn dequantize(d: u64, state: &mut MbeDecoderState) -> DequantizedFrame`
**See also:** test `fixed_dequantize_matches_float_across_representative_sequences` in `tests/ambe_fixed_dstar.rs`; `float_dstar_decode_classify_b0`.

### `classify_tone_index` (fixed D-STAR) [@ANCHOR: ham_digital_modes:fixed_dstar_decode_classify_tone_index]

Maps the 8-bit tone index of a D-STAR tone frame to its kind using mbelib's range table: 5 to 122 is a single tone, 128 to 163 a dual tone (DTMF is one), anything else invalid. The single tone
frequency is exact in Q16.16, `index * 31.25` Hz (`HZ_PER_INDEX_Q16_16`). It is the fixed twin of the float function and must agree with it for every index.

| Argument | Type | Meaning |
|---|---|---|
| `index` | `u32` | Tone index extracted by `decode_tone`, 0 to 255 |

**Returns:** `ToneKind`: `Invalid`, `Single { hz_q16: i32 }` or `Dual`.
**Errors:** none; unknown indices return `Invalid`.
**Special concerns:** pure.
**Signature:** `pub fn classify_tone_index(index: u32) -> ToneKind`
**See also:** test `classify_tone_index_hz_matches_the_float_sibling_exactly`; `float_dstar_decode_classify_tone_index`.

### `quantize_pitch_p8` (fixed AMBE+2) [@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_encode_quantize_pitch_p8]

Returns `b0` for a pitch period of `p8 / 8` samples: the integer port of the float `quantize_pitch`. It converts the period to `omega0 = 2 pi 8 / p8` in Q32 radians per sample
and returns the index of the nearest of the first 120 entries of `W0_TABLE_Q32` (first of equals). The streaming AMBE+2 encoder calls it after pitch refinement.

| Argument | Type | Meaning |
|---|---|---|
| `p8` | `u32` | Pitch period in eighths of a sample; 0 is treated as 1 |

**Returns:** `u32`, `b0` in 0 to 119.
**Errors:** none.
**Special concerns:** a linear scan of 120 entries; pure.
**Signature:** `pub fn quantize_pitch_p8(p8: u32) -> u32`
**See also:** test `pitch_quantizer_matches_the_float_one_for_every_period`; `float_dstar_quantize_quantize_pitch`.

### `quantize_pitch_p8` (fixed D-STAR) [@ANCHOR: ham_digital_modes:fixed_dstar_encode_quantize_pitch_p8]

Returns `b0` for a pitch period of `p8 / 8` samples by inverting the chip's pitch map `f0 = 2^(-4.258618 - 0.021766 b0)`. With `f0 = 8 / p8`,
`b0 = (log2(p8) - 3 - 4.258618) / 0.021766`, evaluated in Q16 with a table-interpolated `log2`, rounded to nearest and clamped to 0 to 125 (126 and 127 are the tone codes).

| Argument | Type | Meaning |
|---|---|---|
| `p8` | `u32` | Pitch period in eighths of a sample; 0 is treated as 1 |

**Returns:** `u32`, `b0` in 0 to 125.
**Errors:** none.
**Special concerns:** pure; integer only.
**Signature:** `pub fn quantize_pitch_p8(p8: u32) -> u32`
**See also:** test `pitch_quantizer_matches_the_float_one_for_every_period`; `float_dstar_quantize_quantize_pitch`.

### `classify_tone_index` (float D-STAR) [@ANCHOR: ham_digital_modes:float_dstar_decode_classify_tone_index]

Classifies a D-STAR tone frame's index per mbelib's range table: 5 to 122 is `Single { hz: index * 31.25 }`, 128 to 163 is `Dual`, and everything else (0 to 4, 123 to 127, 164 and up) is `Invalid`. A live
chip capture of a 200 Hz tone produced index 6, which reads as 187.5 Hz, one quantization step (31.25 Hz) below the stimulus.

| Argument | Type | Meaning |
|---|---|---|
| `index` | `u32` | Tone index from `decode_tone` |

**Returns:** `ToneKind` with an `f64` frequency for single tones.
**Errors:** none.
**Special concerns:** pure; reference for the fixed version.
**Signature:** `pub fn classify_tone_index(index: u32) -> ToneKind`
**See also:** test `classify_tone_index_hz_matches_the_float_sibling_exactly`; `fixed_dstar_decode_classify_tone_index`.

### `classify_b0` (float D-STAR) [@ANCHOR: ham_digital_modes:float_dstar_decode_classify_b0]

Separates tone frames from speech frames by the pitch index `b0`: `b0 & 0x7E == 0x7E` (126 or 127) is `Tone`, everything else `Speech`. D-STAR, matching mbelib exactly, does not treat 120 or 121 as special:
a DTX silence frame at `b0 = 120` decodes as ordinary speech by the reference decoder too. Called by both `dequantize` functions and by the tone round-trip tests.

| Argument | Type | Meaning |
|---|---|---|
| `b0` | `u32` | Pitch/mode index, 0 to 127 |

**Returns:** `FrameKind::Speech` or `FrameKind::Tone`.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn classify_b0(b0: u32) -> FrameKind`
**See also:** tests `classify_b0_matches_mbelib_exactly_not_the_wider_ambe_plus_2_range`, `tone_round_trip_dtmf_and_one_kilohertz`.

### `classify_b0` (float AMBE+2) [@ANCHOR: ham_digital_modes:float_ambe_plus_2_decode_classify_b0]

The AMBE+2 pitch-index classifier: 0 to 119 `Speech`, 120 `DetectedTone`, 121 and 123 `Erasure`, 122 `CallProgress`, 124 and 125 `Silence`, 126 and up `Tone`. Sub-codes 120 and 122 are tone-bearing
(recoverable through the tone-index decoders) rather than true erasures, as confirmed against live chip captures. 126 and 127 (`Tone`) were never observed from the chip and are kept for spec completeness.

| Argument | Type | Meaning |
|---|---|---|
| `b0` | `u32` | Pitch/mode index |

**Returns:** `FrameKind`.
**Errors:** none; every value maps to a kind.
**Special concerns:** pure.
**Signature:** `pub fn classify_b0(b0: u32) -> FrameKind`
**See also:** test `classify_b0_separates_tone_bearing_subcodes_from_genuine_erasure`; `fixed_ambe_plus_2_decode_dequantize`.

### `quantize_pitch` (float D-STAR) [@ANCHOR: ham_digital_modes:float_dstar_quantize_quantize_pitch]

Returns `b0`, the pitch index nearest a target fundamental `w0` (radians per sample): the decoder's `f0`/`w0` formula run in reverse. It converts `f0 = w0 / 2 pi`, estimates `b0` with
`decode::b0_from_f0`, then rounds and clamps to 0 to 125. The fixed `quantize_pitch_p8` must equal it for every pitch period.

| Argument | Type | Meaning |
|---|---|---|
| `w0` | `f64` | Target fundamental in radians per sample; must be finite and positive |

**Returns:** `u32`, `b0` in 0 to 125.
**Errors:** panics (assert) when `w0` is not finite and positive.
**Special concerns:** pure; float.
**Signature:** `pub fn quantize_pitch(w0: f64) -> u32`
**See also:** test `pitch_quantizer_matches_the_float_one_for_every_period`; `fixed_dstar_encode_quantize_pitch_p8`.

### `frame_to_wire_bytes` [@ANCHOR: ham_digital_modes:float_dstar_interleave_frame_to_wire_bytes]

The exact inverse of `wire_bytes_to_frame`: it takes the logical 72-bit D-STAR frame (`C0(24)||C1(23)||C2(11)||C3(14)`, each block MSB-first in the low 72 bits) and scatters each bit through the
`dW`/`dX` interleave tables into 9 chip wire bytes, each byte filled LSB-first. Used when the encoder emits frames to the chip or a D-STAR stream.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `u128` | Logical frame; only the low 72 bits are used |

**Returns:** `[u8; 9]`, the wire bytes in chip order.
**Errors:** none; bits above 72 are masked off.
**Special concerns:** pure.
**Signature:** `pub fn frame_to_wire_bytes(frame: u128) -> [u8; 9]`
**See also:** test `wire_bytes_and_frame_round_trip`; `wire_bytes_to_frame`.

### `wire_bytes_to_frame` [@ANCHOR: ham_digital_modes:float_dstar_interleave_wire_bytes_to_frame]

Converts 9 raw CHAND bytes (chip wire order: bytes as received, each byte's bits LSB-first) into the crate's logical 72-bit frame value `C0(24)||C1(23)||C2(11)||C3(14)`, each block packed
MSB-first, the input convention of `decode::parse_frame`. It places wire bit `i` at row `dW[i]`, column `dX[i]` of a 4 by 24 bit matrix and reads the blocks back out.

| Argument | Type | Meaning |
|---|---|---|
| `bytes` | `&[u8; 9]` | Wire bytes as received |

**Returns:** `u128` holding the 72-bit logical frame.
**Errors:** none.
**Special concerns:** pure; a test checks that `dW`/`dX` partition the 72 bits into the real block sizes.
**Signature:** `pub fn wire_bytes_to_frame(bytes: &[u8; 9]) -> u128`
**See also:** test `wire_bytes_and_frame_round_trip`; `frame_to_wire_bytes`.

### `build_frame` [@ANCHOR: ham_digital_modes:float_dstar_encode_build_frame]

Builds the full transmittable 72-bit logical frame from the 49-bit `d[]` layout: Golay-encodes `C0`'s 12 data bits (the 24th bit is the even parity of the codeword, which decoders here never check),
Golay-encodes and whitens `C1`'s 12 data bits with `C0`'s data as the whitening seed, and carries `C2` (11 bits) and `C3` (14 bits) raw. The result is packed MSB-first and goes to
`frame_to_wire_bytes`.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `u64` | The 49-bit parameter word: `C0` data bits 48..37, `C1` data 36..25, `C2` 24..14, `C3` 13..0 |

**Returns:** `u128`, the logical frame in the low 72 bits.
**Errors:** none; bits of `d` above 49 are ignored by the masks.
**Special concerns:** pure.
**Signature:** `pub fn build_frame(d: u64) -> u128`
**See also:** test `build_frame_round_trips_through_real_wire_bytes`; `c0_field_has_even_parity_like_the_chips_extended_golay_code`.

### `update_xi_max` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_update_xi_max]

Updates the running maximum of the frame spectral energy `xi_max` (Eq. 41), Q16, used by the voicing threshold. If the new energy `xi_0` exceeds the previous maximum, the result is the midpoint of the two;
otherwise it decays as `(99 xi_max_prev + xi_0) / 100`, never below `XI_MAX_FLOOR_Q16` (20000 in Q16). Called by `determine_voicing` once per frame.

| Argument | Type | Meaning |
|---|---|---|
| `xi_max_prev` | `i128` | Previous maximum, Q16 |
| `xi_0` | `i128` | This frame's `xi_LF + xi_HF`, Q16 |

**Returns:** `i128`, the new `xi_max`, Q16.
**Errors:** none.
**Special concerns:** pure; the stream starts at `XI_MAX_INITIAL_Q16` (the floor).
**Signature:** `pub fn update_xi_max(xi_max_prev: i128, xi_0: i128) -> i128`
**See also:** test `xi_max_update_and_energy_dependent_function_match_the_equations`; `fixed_tia_102_baba_vuv_determine_voicing`.

*(Reference: `src/ambe/dvsi_p25fec/fec.rs`; `src/ambe/fixed/ambe_plus_2/decode.rs`; `src/ambe/fixed/dstar/decode.rs`; `src/ambe/fixed/ambe_plus_2/encode.rs`; `src/ambe/fixed/dstar/encode.rs`; `src/ambe/float/dstar/decode.rs`; `src/ambe/float/ambe_plus_2/decode.rs`; `src/ambe/float/dstar/quantize.rs`; `src/ambe/float/dstar/interleave.rs`; `src/ambe/float/dstar/encode.rs`; `src/ambe/fixed/tia_102_baba/vuv.rs`)*

## Fixed-point TIA-102 analysis chain anchored in slice 6 (pitch tracking and refinement, voicing, amplitudes)

The encoder's analysis side is tested end to end against its float sibling on real speech, and each step is cited by those tests. `PitchAnalysisFrame` builds the pitch error table
E(P) for the 203 candidate periods (21.0 to 122.0 samples in half-sample steps; a silent frame has unit error); `look_back_pitch_tracking`, `look_ahead_pitch_tracking` and
`choose_initial_pitch_estimate` pick the initial estimate; `RefinementFrame`, `window_dft_16384_q22`, `harmonic_amplitude_q16`, `spectrum_error_and_energy`, `refinement_error` and
`refine_pitch` refine it to an eighth of a sample; `xi_over`, `voicing_threshold_q30`, `voicing_measure_q30` and `determine_voicing` make the per-band voiced/unvoiced decisions (Eq. 35 to 41);
`estimate_spectral_amplitudes_q16`, `voiced_amplitude_q16` and `unvoiced_amplitude_q16` produce the harmonic amplitudes; `analyze_at_pitch` ties them together. All are pure integer
functions with no I/O, locks or transmitter involvement.

### `PitchAnalysisFrame::new` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_PitchAnalysisFrame_new]

Builds the per-frame analysis state for the pitch error function. It applies the Eq. 9 lowpass (21 taps) to the 301 samples around the center in a Q30 accumulator, block-normalises so the peak lies in
`[2^21, 2^22)`, forms the windowed energy `S`, and the table of integer-lag autocorrelations `R[t]` (about 34 thousand multiply-accumulates, once per frame). An all-zero frame is marked `silent` with zero energy.

| Argument | Type | Meaning |
|---|---|---|
| `raw` | `&[i32]` | PCM samples at 16-bit scale |
| `center` | `usize` | Index of the window center; the function reads `raw[center-160..=center+160]` |

**Returns:** `PitchAnalysisFrame` (`s_energy`, 151 autocorrelation lags, `silent` flag).
**Errors:** panics on an out-of-range index if the slice does not contain the window (caller must supply it).
**Special concerns:** pure; deterministic; the largest cost per frame in the pitch stage.
**Signature:** `pub fn new(raw: &[i32], center: usize) -> Self`
**See also:** test `silent_frame_has_unit_error_and_default_history_index_is_pitch_100`; `error_function`.

### `PitchAnalysisFrame::error_function` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_PitchAnalysisFrame_error_function]

Computes `E(P)` for one candidate index as Q16.16: `(S - P Rsum) / (S (1 - P W4))`, where `Rsum` sums the autocorrelation at lags `0` and every multiple `n P` up to 300 samples (half-sample lags average the
two neighbouring integer lags). An all-zero frame scores the worst error, 1.0 (65536), as the float sibling does; the result is clamped to be at least zero, and a zero denominator also returns 1.0.

| Argument | Type | Meaning |
|---|---|---|
| `idx` | `usize` | Candidate index, 0 to 202 (`P = 21 + idx / 2`) |

**Returns:** `i32`, `E(P)` in Q16.16.
**Errors:** none returned; an index above 202 is not validated here (unknown whether it panics, to-do verify).
**Special concerns:** pure, uses `i128` internally.
**Signature:** `pub fn error_function(&self, idx: usize) -> i32`
**See also:** test `silent_frame_has_unit_error_and_default_history_index_is_pitch_100`; `error_table`.

### `PitchAnalysisFrame::error_table` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_PitchAnalysisFrame_error_table]

Evaluates `error_function` for every one of the 203 candidates and returns them as an `ErrorTable` (`[i32; 203]`, Q16.16). The tracking functions and the initial-estimate choice read the table of the current
and the next two frames. Measured against the float sibling on real speech, the maximum absolute error over all candidates is 0.00002 (as documented in the module header).

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&PitchAnalysisFrame` | The frame state from `new` |

**Returns:** `ErrorTable`.
**Errors:** none.
**Special concerns:** pure; 203 calls to `error_function`.
**Signature:** `pub fn error_table(&self) -> ErrorTable`
**See also:** tests `fixed_pitch_tracking_matches_float_on_real_speech`, `silent_frame_has_unit_error_and_default_history_index_is_pitch_100`.

### `choose_initial_pitch_estimate` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_choose_initial_pitch_estimate]

Section 5.1.4: chooses between the backward estimate and the forward estimate and returns the pitch candidate index. It returns the backward index when its cumulative error `CE_B` is at most 0.48 (Q16.16) or
not greater than the forward `CE_F`, else the forward index.

| Argument | Type | Meaning |
|---|---|---|
| `idx_b`, `ce_b` | `usize`, `i32` | Look-back index and cumulative error, Q16.16 |
| `idx_f`, `ce_f` | `usize`, `i32` | Look-ahead index and cumulative error, Q16.16 |

**Returns:** `usize`, the chosen candidate index.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn choose_initial_pitch_estimate(idx_b: usize, ce_b: i32, idx_f: usize, ce_f: i32) -> usize`
**See also:** tests `fixed_pitch_tracking_matches_float_on_real_speech`, `ambe_fixed_tia_102_baba_pitch_refinement.rs`; `look_back_pitch_tracking`.

### `look_ahead_pitch_tracking` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_look_ahead_pitch_tracking]

Look-ahead tracking (5.1.3) over the error tables of the current and next two frames. It finds the candidate `P0` minimising `E0 + min E1 + min E2` where each next pitch is within 0.8 to 1.2 of the previous
one, then tests sub-multiples of the winner (smallest pitch first, snapped to the half-sample grid) against the three acceptance rules (thresholds 0.85 with ratio 1.7, 0.4 with ratio 3.5, and 0.05), using
multiplied-out ratio tests so no division is needed. The first sub-multiple that passes replaces the winner.

| Argument | Type | Meaning |
|---|---|---|
| `t0`, `t1`, `t2` | `&ErrorTable` | Error tables of the current, next and next-but-one frames |

**Returns:** `(usize, i32)`: the index of `P_hat_F` and its cumulative error `CE_F`, Q16.16.
**Errors:** none.
**Special concerns:** pure; precomputes best values for all 203 candidates (dynamic programming), so it is cheap.
**Signature:** `pub fn look_ahead_pitch_tracking(t0, t1, t2: &ErrorTable) -> (usize, i32)`
**See also:** test `fixed_pitch_tracking_matches_float_on_real_speech`; `choose_initial_pitch_estimate`.

### `look_back_pitch_tracking` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_look_back_pitch_tracking]

Look-back tracking (5.1.2): among candidates within 0.8 to 1.2 of the previous frame's pitch, picks the one with the smallest current error and returns it with the cumulative error
`CE_B = E + prev1 error + prev2 error` (saturated to `i32`). The range is evaluated exactly in half samples. The defaults for the first frames are `(DEFAULT_PITCH_INDEX, 0)`, that is pitch 100.

| Argument | Type | Meaning |
|---|---|---|
| `table` | `&ErrorTable` | This frame's error table |
| `prev1`, `prev2` | `(usize, i32)` | Final (pitch index, error) of the previous two frames |

**Returns:** `(usize, i32)`: index of `P_hat_B` and `CE_B`.
**Errors:** none.
**Special concerns:** pure; the first candidate with the smallest error wins ties.
**Signature:** `pub fn look_back_pitch_tracking(table, prev1, prev2) -> (usize, i32)`
**See also:** test `fixed_pitch_tracking_matches_float_on_real_speech`; `fixed_tia_102_baba_pitch_look_ahead_pitch_tracking`.

### `RefinementFrame::new` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_RefinementFrame_new]

Computes the 256-point windowed DFT `S_w(m)` of one frame, `m = -127..=128`, Q10. It multiplies the 221 samples around the center by the refinement window, then evaluates the DFT directly against a
Q30 cosine table (rounding each bin); negative bins are the conjugates of the positive ones, since the input is real.

| Argument | Type | Meaning |
|---|---|---|
| `raw` | `&[i32]` | PCM samples |
| `center` | `usize` | Window center; reads `raw[center-110..=center+110]` |

**Returns:** `RefinementFrame` holding 256 complex bins.
**Errors:** panics on an out-of-range index if the window is not inside `raw`.
**Special concerns:** pure; about 29 thousand multiply-accumulates per frame.
**Signature:** `pub fn new(raw: &[i32], center: usize) -> Self`
**See also:** test in `tests/ambe_fixed_tia_102_baba_pitch_refinement.rs`; `sw_at`.

### `RefinementFrame::sw_at` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_RefinementFrame_sw_at]

Reads bin `S_w(m)` (Q10 complex) from the frame's DFT, returning zero outside `-127..=128`. It makes the band loops in the refinement, voicing and amplitude code safe to run past the ends of the table.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `i32` | Bin index |

**Returns:** `Cplx` (`i64` real and imaginary, Q10); zero outside the range.
**Errors:** none.
**Special concerns:** pure, constant time.
**Signature:** `pub fn sw_at(&self, m: i32) -> Cplx`
**See also:** test in `tests/ambe_fixed_tia_102_baba_pitch_refinement.rs`; `RefinementFrame::new`.

### `window_dft_16384_q22` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_window_dft_16384_q22]

Looks up `W_R(m)`, the refinement window's DFT sampled at 16384 points, Q22, from `WINDOW_DFT_16384_Q22`. The table covers `|m| <= WINDOW_DFT_HALF_RANGE` (512); anything outside is
treated as zero. The reachable range for pitches of at least 19.875 is about 476, so the cutoff is never hit in practice.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `i32` | Window-DFT index (from `Pitch::window_index`) |

**Returns:** `i64`, `W_R(m)` in Q22; 0 outside the table.
**Errors:** none.
**Special concerns:** pure; the float sibling's `window_dft_16384_direct` computes the same function directly.
**Signature:** `pub fn window_dft_16384_q22(m: i32) -> i64`
**See also:** test in `tests/ambe_fixed_tia_102_baba_pitch_refinement.rs`; `harmonic_amplitude_q16`.

### `harmonic_amplitude_q16` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_harmonic_amplitude_q16]

Estimates the complex amplitude `A_l` of harmonic `l` (Eq. 26 to 28): `sum(S_w(m) W_R(.)) / sum(W_R^2)` over the bins from `ceil(a_l)` up to `ceil(b_l)` (`Pitch::band_start`). The numerator is Q32 and
the denominator Q44, both `i128`; the quotient is Q16 complex. If the denominator is zero (empty band) it returns zero.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | The windowed DFT |
| `l` | `u32` | Harmonic number |
| `pitch` | `&Pitch` | Exact-fraction pitch (from `P8` or `omega0`) |

**Returns:** `Cplx`, Q16.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn harmonic_amplitude_q16(frame: &RefinementFrame, l: u32, pitch: &Pitch) -> Cplx`
**See also:** test `fixed_pitch_refinement_matches_float_on_real_speech`; `spectrum_error_and_energy`.

### `spectrum_error_and_energy` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_spectrum_error_and_energy]

Over the bins `m_start <= m < m_end`, returns `(err, real)`: `real = sum |S_w(m)|^2` and `err = sum |S_w(m) - S_synth(m)|^2`, where the synthetic spectrum is built from harmonics `0..=max_l` at the given
pitch (Eq. 25); a bin in no harmonic's band has synthetic value 0. Both are Q60 `i128`. It is the shared core of `refinement_error` (the `err` part) and `voicing_measure_q30` (their ratio).

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `pitch` | `&Pitch` | Candidate pitch |
| `max_l` | `u32` | Highest harmonic included |
| `m_start`, `m_end` | `i32` | Bin range, end exclusive |

**Returns:** `(i128, i128)`: `(err, real)`, Q60.
**Errors:** none.
**Special concerns:** pure; recomputes `harmonic_amplitude_q16` for each harmonic overlapping the range.
**Signature:** `pub fn spectrum_error_and_energy(frame, pitch, max_l, m_start, m_end) -> (i128, i128)`
**See also:** test `fixed_pitch_refinement_matches_float_on_real_speech`; `refinement_error`.

### `refine_pitch` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_refine_pitch]

Refines the half-sample initial pitch candidate to eighth-sample accuracy (the doc comment says quarter-sample, the offsets are eighths): it tries `P = p_hat_i + {-9,-7,-5,-3,-1,1,3,5,7,9}/8`, evaluates
`refinement_error` for each and returns the `P8` with the smallest `E_R` (first of equals, as the float sibling's `min_by`). On real speech it equals the float result on 99.99% of frames.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Windowed DFT of the frame |
| `p_hat_i_index` | `usize` | Initial candidate index from the tracking stage (`P = 21 + index / 2`) |

**Returns:** `u32`, the refined period in eighths of a sample.
**Errors:** none.
**Special concerns:** pure; ten full error evaluations per call, the dominant cost of the refinement stage.
**Signature:** `pub fn refine_pitch(frame: &RefinementFrame, p_hat_i_index: usize) -> u32`
**See also:** test `fixed_pitch_refinement_matches_float_on_real_speech`; `refinement_error`.

### `refinement_error` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_refinement_error]

The refinement error `E_R` (Eq. 24) at a given pitch, Q60. It takes `L_est` from the pitch (`floor(0.9254 P / 2 - 1/2)`), the upper bin, and sums the spectral error over bins 50 up to that bin for
harmonics up to `L_est + 1`, via `spectrum_error_and_energy`. `refine_pitch` minimises it.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `pitch` | `&Pitch` | Pitch under test |

**Returns:** `i128`, `E_R`, Q60.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn refinement_error(frame: &RefinementFrame, pitch: &Pitch) -> i128`
**See also:** test `fixed_pitch_refinement_matches_float_on_real_speech`; `refine_pitch`.

### `determine_voicing` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_determine_voicing]

Decides voiced or unvoiced for each frequency band and updates `xi_max`. It computes `L_hat` and `K_hat` from the pitch, the low- and high-frequency energies (`xi_lf`, `xi_hf`), updates `xi_max`
(`update_xi_max`), the energy-dependent function `M(xi)`, then for each band `k` declares it voiced when the measure `D_k` is below the threshold `Theta_xi(k)`. The previous frame's decision for the
same band feeds the threshold; missing entries count as unvoiced.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `pitch` | `&Pitch` | Refined pitch |
| `initial_pitch_error_q16` | `i32` | `E(P_hat_I)`, Q16.16 |
| `xi_max_prev_q16` | `i128` | Running maximum from the previous frame |
| `previous_v` | `&[bool]` | Previous frame's band decisions; empty for the first frame |

**Returns:** `(Vec<bool>, i128)`: one flag per band (`K_hat` entries) and the new `xi_max`, Q16.
**Errors:** none.
**Special concerns:** pure; allocates the result vector.
**Signature:** `pub fn determine_voicing(frame, pitch, initial_pitch_error_q16, xi_max_prev_q16, previous_v) -> (Vec<bool>, i128)`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `voicing_measure_q30`, `voicing_threshold_q30`.

### `voicing_measure_q30` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_voicing_measure_q30]

The band voicing measure `D_k` (Eq. 35 and 36) in Q30: the ratio of the synthetic-spectrum error to the real energy over the band's bins, from `spectrum_error_and_energy` and a wide-range `ratio_q30`. Band `k` covers
harmonics `3k-2` to `3k` (the highest band extends to `L_hat`). A band with no spectral energy counts as maximally unvoiced (1.0), as in the float sibling.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `k` | `u32` | Band number, from 1 |
| `l_hat` | `u32` | Harmonic count |
| `pitch` | `&Pitch` | Pitch |
| `is_highest_band` | `bool` | The band extends to `L_hat` instead of `3k` |

**Returns:** `i64`, `D_k` in Q30; `ONE_Q30` when the band has no energy.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn voicing_measure_q30(frame, k, l_hat, pitch, is_highest_band) -> i64`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `determine_voicing`.

### `voicing_threshold_q30` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_voicing_threshold_q30]

The voicing threshold `Theta_xi(k)` (Eq. 37) in Q30. If the initial pitch error exceeds 0.5 and `k >= 2` the threshold is zero (the band is forced unvoiced). Otherwise it is `M(xi) * (1 - 0.3096 (k-1) omega0)`,
scaled by 9/16 when the previous frame's band was voiced and by 9/20 when it was not.

| Argument | Type | Meaning |
|---|---|---|
| `k` | `u32` | Band number from 1 |
| `omega0_q30` | `i64` | Fundamental in radians per sample, Q30 |
| `initial_pitch_error_q16` | `i32` | `E(P_hat_I)`, Q16.16 |
| `previous_band_voiced` | `bool` | Previous frame's decision for this band |
| `m_xi_q30` | `i64` | Energy-dependent function `M(xi)`, Q30 |

**Returns:** `i64`, the threshold in Q30.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn voicing_threshold_q30(k, omega0_q30, initial_pitch_error_q16, previous_band_voiced, m_xi_q30) -> i64`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `determine_voicing`.

### `xi_over` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_xi_over]

Private helper giving the normalised spectral energy `sum |S_w(m)|^2 / W_R(0)^2` over bins `lo..=hi`, Q16. `xi_lf` uses bins 0 to 63 (Eq. 38) and `xi_hf` bins 64 to 128 (Eq. 39); their sum is `xi_0` used by
`determine_voicing`. The sum is in Q20 and shifted by 40 before dividing by the squared window DC gain in Q44.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `lo`, `hi` | `i32` | Inclusive bin range |

**Returns:** `i128`, Q16.
**Errors:** none (a zero window DC gain would divide by zero; the table value is nonzero).
**Special concerns:** pure; private.
**Signature:** `fn xi_over(frame: &RefinementFrame, lo: i32, hi: i32) -> i128`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `update_xi_max`.

### `estimate_spectral_amplitudes_q16` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_spectral_amplitude_estimate_spectral_amplitudes_q16]

Returns `M_hat_l` for `l = 1..=l_hat` in Q16, using the voiced estimator (`voiced_amplitude_q16`) for harmonics whose band was decided voiced and the unvoiced estimator (`unvoiced_amplitude_q16`) otherwise.
The band of harmonic `l` is `ceil(l / 3)` limited to `K_hat`; a band index missing from `voiced` counts as unvoiced.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `l_hat` | `u32` | Harmonic count |
| `k_hat` | `u32` | Band count |
| `pitch` | `&Pitch` | Pitch |
| `voiced` | `&[bool]` | Band decisions from `determine_voicing` |

**Returns:** `Vec<i64>` of `l_hat` amplitudes, Q16 (about `3.3e4 * 2^16` for a full-scale sine, hence `i64`).
**Errors:** none.
**Special concerns:** pure; allocates.
**Signature:** `pub fn estimate_spectral_amplitudes_q16(frame, l_hat, k_hat, pitch, voiced) -> Vec<i64>`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `analyze_at_pitch`.

### `voiced_amplitude_q16` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_spectral_amplitude_voiced_amplitude_q16]

The voiced amplitude `A_voiced(l)` (Eq. 43), Q16: `sqrt(E_sig / E_win)` over the harmonic's band, where `E_sig = sum |S_w(m)|^2` (Q20) and `E_win = sum W_R^2` (Q44). The root is an integer square root
of `E_sig * 2^56 / E_win`, so no precision is lost before it.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `l` | `u32` | Harmonic |
| `pitch` | `&Pitch` | Pitch |

**Returns:** `i64`, Q16; 0 when the window energy is zero.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn voiced_amplitude_q16(frame: &RefinementFrame, l: u32, pitch: &Pitch) -> i64`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `unvoiced_amplitude_q16`.

### `unvoiced_amplitude_q16` [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_spectral_amplitude_unvoiced_amplitude_q16]

The unvoiced amplitude `A_unvoiced(l)` (Eq. 44), Q16: `sqrt(E_sig / width) / sum(w_R)`, where `width` is the number of bins in the band and `sum(w_R)` equals the window DC gain `W_R(0)` (Q22).
Implemented as `isqrt(E_sig_q20 * 2^12 / width) * 2^22 / W_R(0)`.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame` | Measured spectrum |
| `l` | `u32` | Harmonic |
| `pitch` | `&Pitch` | Pitch |

**Returns:** `i64`, Q16; 0 for an empty band.
**Errors:** none.
**Special concerns:** pure.
**Signature:** `pub fn unvoiced_amplitude_q16(frame: &RefinementFrame, l: u32, pitch: &Pitch) -> i64`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `voiced_amplitude_q16`.

### `analyze_at_pitch` (fixed) [@ANCHOR: ham_digital_modes:fixed_mbe_encode_analyze_at_pitch]

Per-harmonic voicing (`voiced[h]`, index 0 unused) and amplitudes (`ml[h]`, Q16, index 0 unused) for harmonics `1..=l` at a pitch, mirroring the float `analyze_at_pitch` exactly. It calls `determine_voicing`,
stores the new `xi_max` and band decisions in the state, pads the bands to `K_hat(l)` with the last decision, then lets each harmonic take its band's flag and amplitude from
`estimate_spectral_amplitudes_q16`.

| Argument | Type | Meaning |
|---|---|---|
| `refinement` | `&RefinementFrame` | Measured spectrum |
| `initial_pitch_error_q16` | `i32` | `E(P_hat_I)`, Q16.16 |
| `pitch` | `&Pitch` | Pitch to analyse at (the refined pitch or a decoder-quantized one) |
| `l` | `u32` | Harmonic count |
| `state` | `&mut AnalysisState` | `xi_max_q16` and previous band decisions; updated |

**Returns:** `(Vec<bool>, Vec<i64>)`, each of length `l + 1`.
**Errors:** none.
**Special concerns:** mutates `state`, so frames must be analysed in order, once each.
**Signature:** `pub fn analyze_at_pitch(refinement, initial_pitch_error_q16, pitch, l, state) -> (Vec<bool>, Vec<i64>)`
**See also:** test `fixed_voicing_and_amplitudes_match_float_on_real_speech_q30_omega0`; `fixed_mbe_encode_analyze_and_quantize`.

*(Reference: `src/ambe/fixed/tia_102_baba/pitch.rs`; `src/ambe/fixed/tia_102_baba/pitch_refinement.rs`; `src/ambe/fixed/tia_102_baba/vuv.rs`; `src/ambe/fixed/tia_102_baba/spectral_amplitude.rs`; `src/ambe/fixed/mbe_encode.rs`)*

## Fixed and float AMBE codec chain anchored in slice 7 (encoders, decoders, synthesis, concealment)

The streaming encoders take PCM and return frames: `push_samples`, then `next_frame` until it returns `None`, then `finish`. They are in-memory
state machines (no I/O, no locks, no threads; a single owner calls them in order), so no transmitter keying, PTT, band check or transmit gate applies here; whatever carries the
frames to a radio or chip does that. Each fixed encoder has a float sibling with the same surface, and the parity tests run both on real speech.

### `Encoder::new` (fixed D-STAR) [@ANCHOR: ham_digital_modes:fixed_dstar_encoder_Encoder_new]

Creates the streaming fixed-point D-STAR encoder: a `FrameAnalyzer` whose analysis-center offset is set to `CHIP_ALIGNED_CENTER_OFFSET` (-80 samples, the offset at which its pitch track lines up with the chip's),
a default `HighPassFilter`, a mirror of the fixed decoder state (`MbeDecoderState::initial()`) and a fresh `AnalysisState`. The mirror is advanced by dequantizing each emitted frame so the quantizer sees the same
predictor state as a receiving decoder.

| Argument | Type | Meaning |
|---|---|---|
| none | | |

**Returns:** `Encoder`.
**Errors:** none.
**Special concerns:** `set_center_offset` can change the alignment afterwards; `Default` calls `new`.
**Signature:** `pub fn new() -> Self`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames`; `float_dstar_encoder_Encoder_new`.

### `Encoder::next_frame` (fixed D-STAR) [@ANCHOR: ham_digital_modes:fixed_dstar_encoder_Encoder_next_frame]

Returns the next 72-bit logical D-STAR frame once enough look-ahead has been pushed (two frames), else `None`. It takes the frame's analysis from `FrameAnalyzer::next_analysis`; if `detect_tone` finds a tone in the frame's
160 samples it returns a tone frame (DTMF index `128 + row + 4*col`, or the single-tone index); otherwise it quantizes the pitch with `quantize_pitch_p8`, takes `L` from `L_TABLE`, runs `analyze_and_quantize` against the mirror
state, packs `b0..b8` with `pack_raw_parameters`, advances the mirror with `dequantize`, and builds the frame with `build_frame`.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Advances the analyzer, the mirror decoder state and the voicing history |

**Returns:** `Option<u128>`: the logical frame in the low 72 bits (use `frame_to_wire_bytes` for chip wire bytes), or `None` when more input is needed.
**Errors:** none returned.
**Special concerns:** stateful: frames must be pulled in order, and calling it also advances the mirror, so it is not idempotent.
**Signature:** `pub fn next_frame(&mut self) -> Option<u128>`
**See also:** test in `tests/ambe_fixed_dstar_encoder.rs` (`speech_parity_with_the_float_encoder_first_150_frames`); `fixed_dstar_encoder_Encoder_finish`.

### `Encoder::finish` (fixed D-STAR) [@ANCHOR: ham_digital_modes:fixed_dstar_encoder_Encoder_finish]

Flushes the encoder at the end of the input: it calls `FrameAnalyzer::finish_input` (which pads with silence so every pushed sample's frame is complete), then drains `next_frame` into a vector. Call it once after the last
`push_samples`; the returned frames are the ones still held back by the two-frame look-ahead.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Consumed in effect: no more input should be pushed afterwards |

**Returns:** `Vec<u128>`, the remaining logical frames (possibly empty).
**Errors:** none.
**Special concerns:** the silence padding makes the last frames cover silence, so the output count equals the input length rounded up to whole frames of 160 samples (as parity tests check against the float encoder).
**Signature:** `pub fn finish(&mut self) -> Vec<u128>`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames`; `fixed_dstar_encoder_Encoder_next_frame`.

### `Encoder::next_frame` (fixed AMBE+2) [@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_encoder_Encoder_next_frame]

The AMBE+2 half-rate streaming step, same structure as the D-STAR encoder: analysis, tone detection (a tone frame via `build_tone_frame` with the DTMF or single tone index and the amplitude field from `amplitude_field_q16`),
else `quantize_pitch_p8` for `b0`, `analyze_and_quantize` against the mirror, `dequantize(&raw, mirror)` to advance the mirror state, and `build_frame(&raw)`. Returns `None` until the look-ahead is filled.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Advances the analyzer, mirror state and analysis history |

**Returns:** `Option<u128>`, the 72-bit logical frame.
**Errors:** none.
**Special concerns:** stateful, in order; no interleaving into wire bytes (see the AMBE+2 interleave helpers).
**Signature:** `pub fn next_frame(&mut self) -> Option<u128>`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames` in `tests/ambe_fixed_ambe_plus_2_encoder.rs`; `float_ambe_plus_2_encoder_Encoder_next_frame`.

### `Encoder::finish` (fixed AMBE+2) [@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_encoder_Encoder_finish]

Ends an AMBE+2 stream: pads the analyzer with silence through `finish_input` and drains `next_frame`, returning the frames that were waiting on look-ahead. Call once after the last samples.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Further `push_samples` afterwards would start from padded silence |

**Returns:** `Vec<u128>` of 72-bit logical frames.
**Errors:** none.
**Special concerns:** idempotent padding (`finish_input` pads once), so a second call returns an empty vector.
**Signature:** `pub fn finish(&mut self) -> Vec<u128>`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames`; `fixed_ambe_plus_2_encoder_Encoder_next_frame`.

### `Encoder::next_frame` (fixed TIA-102) [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_Encoder_next_frame]

Encodes the next TIA-102.BABA frame as eight code vectors once enough look-ahead has been pushed, else `None`. It takes the frame analysis from `FrameAnalyzer::next_analysis` and calls
`encode_frame` with the refined pitch, the initial pitch error and the previous `FrameState`. On success the state advances and the frame is remembered; if analysis fails (degenerate pitch or `L_hat`)
the failure counter `failed_frames` is incremented and the previous frame is repeated (or all zeros before any frame).

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Advances analyzer and frame state |

**Returns:** `Option<[u32; 8]>`, the eight code vectors, or `None` when more input is needed.
**Errors:** none returned; failures are counted in the public field `failed_frames`.
**Special concerns:** two frames of look-ahead delay; stateful.
**Signature:** `pub fn next_frame(&mut self) -> Option<[u32; 8]>`
**See also:** test `tia_parity_first_150_frames`; `float_tia_102_baba_encoder_Encoder_next_frame`.

### `Encoder::finish` (fixed TIA-102) [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_Encoder_finish]

Pads the analyzer with silence so every pushed sample's frame can be emitted, then drains `next_frame` and returns the remaining frames. Call once at the end of the input.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Stream is finished afterwards |

**Returns:** `Vec<[u32; 8]>`, the remaining code-vector frames.
**Errors:** none.
**Special concerns:** failed frames inside the flush also count in `failed_frames`.
**Signature:** `pub fn finish(&mut self) -> Vec<[u32; 8]>`
**See also:** test `tia_parity_first_150_frames`; `fixed_tia_102_baba_encoder_Encoder_next_frame`.

### `FrameAnalyzer::finish_input` (fixed TIA-102) [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_FrameAnalyzer_finish_input]

Marks the end of the input and pads the analyzer's sample buffer with `3 * 160 + 2 * 160` zeros (silence), once, so the last real frames have the look-ahead and margin they need. After it,
`next_analysis` stops when the next frame's center is beyond the real samples. A second call does nothing.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut FrameAnalyzer` | Sets the `finished` flag |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** do not push samples afterwards.
**Signature:** `pub fn finish_input(&mut self)`
**See also:** test `streaming_fixed_analyzer_matches_the_float_analyzer_on_real_speech`; `next_analysis`.

### `FrameAnalyzer::next_analysis` (fixed TIA-102) [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_FrameAnalyzer_next_analysis]

Produces the analysis of the next frame, or `None` if more input is needed (or the stream is exhausted after `finish_input`). It builds (and caches) the pitch error tables for frames `k`, `k+1`, `k+2`, runs the look-back and
look-ahead tracking with the previous two frames' results, chooses the initial pitch, refines it to eighth samples, and returns a `FrameAnalysis` with `p8`, the exact `Pitch`, `initial_pitch_error_q16`, the `RefinementFrame` and the
frame's own 160 `slot_samples` (used by tone detection). It also drops tables and input samples no longer needed.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut FrameAnalyzer` | Holds the sample buffer, error tables and pitch history |

**Returns:** `Option<FrameAnalysis>`.
**Errors:** none; `None` means wait for more samples.
**Special concerns:** stateful and in-order; trims the buffer, so earlier frames cannot be re-analysed.
**Signature:** `pub fn next_analysis(&mut self) -> Option<FrameAnalysis>`
**See also:** test `streaming_fixed_analyzer_matches_the_float_analyzer_on_real_speech`; `fixed_tia_102_baba_pitch_look_ahead_pitch_tracking`.

### `FrameAnalyzer::table` (fixed TIA-102) [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_FrameAnalyzer_table]

Private helper that ensures the pitch error table for frame `k` is in the analyzer's cache. If it is absent it computes the frame's center (`LEAD + k*160 + center_offset`, never below `MARGIN`), builds a
`PitchAnalysisFrame` from the buffered samples and stores its `error_table()`. Frames are tabled once and reused by the three frames that look at them.

| Argument | Type | Meaning |
|---|---|---|
| `k` | `usize` | Frame number |

**Returns:** nothing.
**Errors:** may panic if the buffer does not yet cover the window; `next_analysis` checks `available` first.
**Special concerns:** private; mutates the cache.
**Signature:** `fn table(&mut self, k: usize)`
**See also:** test `streaming_fixed_analyzer_matches_the_float_analyzer_on_real_speech`; `PitchAnalysisFrame::error_table`.

### `HighPassFilter::step` (fixed TIA-102) [@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_HighPassFilter_step]

One sample of the standard's input high-pass filter (Eq. 3), `H(z) = (1 - z^-1) / (1 - 0.99 z^-1)`, in integers: the recursive state is kept in Q16 and the pole `0.99` in Q30, so the result is the exact response rounded to an
integer sample with no accumulating rounding error. It removes DC offset, which would otherwise corrupt the pitch error function. Every streaming encoder calls it on each pushed sample.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut HighPassFilter` | Holds the previous input and previous output (Q16) |
| `x` | `i32` | Input PCM sample |

**Returns:** `i32`, the filtered sample (may exceed 16 bits; callers saturate where they need `i16`).
**Errors:** none.
**Special concerns:** stateful; one filter per stream.
**Signature:** `pub fn step(&mut self, x: i32) -> i32`
**See also:** test `tia_parity_first_150_frames`; `float_tia_102_baba_encoder_HighPassFilter_step`.

### `Encoder::new` (float D-STAR) [@ANCHOR: ham_digital_modes:float_dstar_encoder_Encoder_new]

Creates the float D-STAR streaming encoder: analyzer with center offset `CHIP_ALIGNED_CENTER_OFFSET` (-80, where voiced frames agree with the chip's pitch index within 2 steps 66% of the time),
the high-pass filter, a `DStarDecoderState::initial()` mirror and a fresh `AnalysisState`. It deliberately does not copy the chip's habit of parking all-unvoiced frames on `b0 = 34` (documented in a comment, since it lowers envelope correlation).

| Argument | Type | Meaning |
|---|---|---|
| none | | |

**Returns:** `Encoder`.
**Errors:** none (`push_samples` panics on non-finite samples).
**Special concerns:** input is `&[f64]` whole-number PCM.
**Signature:** `pub fn new() -> Self`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames`; `fixed_dstar_encoder_Encoder_new`.

### `Encoder::next_frame` (float D-STAR) [@ANCHOR: ham_digital_modes:float_dstar_encoder_Encoder_next_frame]

The float D-STAR streaming step and the reference for the fixed one: a tone frame if `detect_tone` fires on the slot samples, else `b0 = quantize_pitch(omega0_hat)`, `L` from `L_TABLE`, `analyze_at_pitch` for voicing and amplitudes,
`quantize_speech` against the mirror state with the D-STAR mode tables, `pack_raw_parameters`, `dequantize` to advance the mirror, and `build_frame`. Returns `None` until look-ahead is available.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Advances analyzer, mirror and analysis state |

**Returns:** `Option<u128>`, the 72-bit logical frame.
**Errors:** none.
**Special concerns:** stateful, in order.
**Signature:** `pub fn next_frame(&mut self) -> Option<u128>`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames`; `fixed_dstar_encoder_Encoder_next_frame`.

### `Encoder::finish` (float D-STAR) [@ANCHOR: ham_digital_modes:float_dstar_encoder_Encoder_finish]

Float D-STAR flush: `finish_input` pads the analyzer with silence, then `next_frame` is drained into a vector. Same contract as the fixed `finish`.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Ends the stream |

**Returns:** `Vec<u128>`.
**Errors:** none.
**Special concerns:** call once after the last `push_samples`.
**Signature:** `pub fn finish(&mut self) -> Vec<u128>`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames`; `fixed_dstar_encoder_Encoder_finish`.

### `Encoder::next_frame` (float AMBE+2) [@ANCHOR: ham_digital_modes:float_ambe_plus_2_encoder_Encoder_next_frame]

Float AMBE+2 step: tone frame via `build_tone_frame` (tone index from `dtmf_tone_idx` or the single-tone index, amplitude field from `amplitude_field`), else `quantize_pitch(omega0_hat)`, `L_TABLE`, `W0_TABLE` for `f0`,
`analyze_at_pitch`, `quantize_speech` with the AMBE+2 mode tables (`hoc_b8_even_only` false, `rho` 0.65), `dequantize` for the mirror and `build_frame`.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Advances analyzer and state |

**Returns:** `Option<u128>`.
**Errors:** none.
**Special concerns:** stateful, in order.
**Signature:** `pub fn next_frame(&mut self) -> Option<u128>`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames` in `tests/ambe_fixed_ambe_plus_2_encoder.rs`; `fixed_ambe_plus_2_encoder_Encoder_next_frame`.

### `Encoder::finish` (float AMBE+2) [@ANCHOR: ham_digital_modes:float_ambe_plus_2_encoder_Encoder_finish]

Float AMBE+2 flush: pad the analyzer with silence and drain `next_frame`, returning the frames still held by the look-ahead.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Ends the stream |

**Returns:** `Vec<u128>`.
**Errors:** none.
**Special concerns:** call once.
**Signature:** `pub fn finish(&mut self) -> Vec<u128>`
**See also:** test `speech_parity_with_the_float_encoder_first_150_frames`; `fixed_ambe_plus_2_encoder_Encoder_finish`.

### `Encoder::next_frame` (float TIA-102) [@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_Encoder_next_frame]

The float TIA-102.BABA encoder step. It records `last_analysis` (initial pitch, refined `omega0_hat`, initial pitch error) for diagnostics and calls `encode_frame`, or `encode_frame_chip_wire` when the encoder was configured
to emit the DVSI chip's framing, with the analysis and previous state. A failed frame increments `failed_frames` and repeats the previous frame (zeros before the first).

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Advances analyzer and frame state |

**Returns:** `Option<[u32; 8]>`, `None` while waiting for look-ahead.
**Errors:** none returned.
**Special concerns:** stateful; chip framing is a configuration choice of the encoder.
**Signature:** `pub fn next_frame(&mut self) -> Option<[u32; 8]>`
**See also:** test `tia_parity_first_150_frames`; `fixed_tia_102_baba_encoder_Encoder_next_frame`.

### `Encoder::finish` (float TIA-102) [@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_Encoder_finish]

Pads the float analyzer with silence and drains `next_frame`, returning the remaining code-vector frames.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Encoder` | Ends the stream |

**Returns:** `Vec<[u32; 8]>`.
**Errors:** none.
**Special concerns:** call once.
**Signature:** `pub fn finish(&mut self) -> Vec<[u32; 8]>`
**See also:** test `tia_parity_first_150_frames`; `fixed_tia_102_baba_encoder_Encoder_finish`.

### `FrameAnalyzer::finish_input` (float TIA-102) [@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_FrameAnalyzer_finish_input]

Pads the float analyzer's buffer with `3 * 160 + 2 * 160` zero samples once, so every pushed sample's frame can be analysed; call once, then drain `next_analysis`. A second call does nothing.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut FrameAnalyzer` | Sets `finished` |

**Returns:** nothing.
**Errors:** none.
**Special concerns:** no pushes afterwards.
**Signature:** `pub fn finish_input(&mut self)`
**See also:** test `streaming_fixed_analyzer_matches_the_float_analyzer_on_real_speech`; `fixed_tia_102_baba_encoder_FrameAnalyzer_finish_input`.

### `FrameAnalyzer::next_analysis` (float TIA-102) [@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_FrameAnalyzer_next_analysis]

Returns the next frame's analysis if enough look-ahead has been pushed (and, after `finish_input`, while frames still cover real input), else `None`. It follows the same procedure as the fixed analyzer in f64: error tables for
three frames, look-back and look-ahead tracking, initial estimate, refinement, and the frame's slot samples for tone detection.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut FrameAnalyzer` | Buffer, tables, pitch history |

**Returns:** `Option<FrameAnalysis>` (float fields such as `initial_pitch`, `omega0_hat`, `initial_pitch_error`).
**Errors:** none.
**Special concerns:** stateful, in order.
**Signature:** `pub fn next_analysis(&mut self) -> Option<FrameAnalysis>`
**See also:** test `streaming_fixed_analyzer_matches_the_float_analyzer_on_real_speech`; `fixed_tia_102_baba_encoder_FrameAnalyzer_next_analysis`.

### `FrameAnalyzer::table` (float TIA-102) [@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_FrameAnalyzer_table]

Private: ensures the pitch error table for frame `k` is cached, computing it with `ErrorTable::compute` on the buffered samples at that frame's center if it is not. Tables are shared by the three frames that need them.

| Argument | Type | Meaning |
|---|---|---|
| `k` | `usize` | Frame number |

**Returns:** nothing.
**Errors:** none expected; `next_analysis` checks availability first.
**Special concerns:** private; mutates the cache.
**Signature:** `fn table(&mut self, k: usize)`
**See also:** test `streaming_fixed_analyzer_matches_the_float_analyzer_on_real_speech`; `fixed_tia_102_baba_encoder_FrameAnalyzer_table`.

### `HighPassFilter::step` (float TIA-102) [@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_HighPassFilter_step]

One sample of the input high-pass filter. `x` is a whole-number PCM value (rounded to `i64`); the arithmetic is the same Q16/Q30 integer recursion as the fixed filter, so the float and fixed encoders see
bit-identical filtered input, and the result is the rounded whole-number output as `f64`. Without the filter a DC offset makes quiet frames look periodic at every lag.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut HighPassFilter` | Filter state |
| `x` | `f64` | Whole-number PCM sample |

**Returns:** `f64`, the filtered sample.
**Errors:** none (NaN input is rejected earlier in `push_samples`).
**Special concerns:** stateful.
**Signature:** `pub fn step(&mut self, x: f64) -> f64`
**See also:** test `tia_parity_first_150_frames`; `fixed_tia_102_baba_encoder_HighPassFilter_step`.

### `decode_frame` (fixed D-STAR synthesis) [@ANCHOR: ham_digital_modes:fixed_dstar_synthesis_DStarSynthesisDecoder_decode_frame]

Decodes one logical 72-bit D-STAR frame into 20 ms of Q16.16 PCM (`N` samples). It runs `parse_frame` (FEC and de-whitening), classifies `b0`, and for speech frames under the default `Concealing` policy delegates
to `decode_concealing`. Otherwise damaged frames (more than 3 corrected errors, or reserved codes 125 and 127 under the chip policy) repeat the previous parameters up to three times, then mute and reinitialize; good frames are
dequantized and synthesized, and tone frames go to the fixed `ToneSynthesizer` (single tone, DTMF, or silence for unidentified dual-tone codes).

| Argument | Type | Meaning |
|---|---|---|
| `logical_frame` | `u128` | 72-bit logical frame (from `wire_bytes_to_frame`) |

**Returns:** `Option<[i64; N]>`, Q16.16 samples; `None` when the synthesizer rejects the parameter lengths or a repeat is requested before any real frame has run.
**Errors:** none returned.
**Special concerns:** stateful (decoder state, synthesizer, repeat counter); one decoder per stream; the policy is set with `with_error_policy`.
**Signature:** `pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[i64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `ham_digital_modes_mbe:ErrorPolicy_is_bad`.

### `decode_concealing` (fixed D-STAR synthesis) [@ANCHOR: ham_digital_modes:fixed_dstar_synthesis_DStarSynthesisDecoder_decode_concealing]

The concealing path of the fixed D-STAR decoder. It dequantizes the frame as received and with each single-bit flip of the 11 correctable raw bits (38 to 48), each against a copy of the state, builds a `Descriptor` for each
speech reading and asks `Concealer::decide` (with the corrected-error count of both code words). `Accept(i)` commits that reading's state and synthesizes it; `Repeat` either resets the decoder and returns silence,
returns silence when the scale is zero, or replays the previous frame faded by the scale.

| Argument | Type | Meaning |
|---|---|---|
| `parsed` | `&ParsedFrame` | Parsed frame: `d`, `epsilon_c0`, `epsilon_c1` |

**Returns:** `Option<[i64; N]>` of Q16.16 PCM.
**Errors:** none returned.
**Special concerns:** private; only for speech frames; the default policy, so the decoder fades instead of muting abruptly.
**Signature:** `fn decode_concealing(&mut self, parsed: &ParsedFrame) -> Option<[i64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `fixed_general_concealment_Concealer_decide`.

### `decode_frame` (fixed AMBE+2 synthesis) [@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_frame]

Decodes one logical 72-bit AMBE+2 frame into 20 ms of Q16.16 PCM. Speech frames use `decode_concealing` under the default policy; otherwise a damaged speech frame repeats (the chip policy still runs the gain recursion on the damaged frame) and after three repeats mutes and
reinitializes. Erasure frames repeat the previous frame, silence frames give zeros, and tone frames (single tone, DTMF, call-progress dial, ring, busy) go through the fixed tone synthesizer; reserved tones give silence.

| Argument | Type | Meaning |
|---|---|---|
| `logical_frame` | `u128` | 72-bit logical frame (from `interleaved_to_frame`) |

**Returns:** `Option<[i64; N]>`, `None` on a rejected parameter length or a repeat before any real frame.
**Errors:** none returned.
**Special concerns:** stateful; one decoder per stream.
**Signature:** `pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[i64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `fixed_ambe_plus_2_decode_dequantize`.

### `decode_concealing` (fixed AMBE+2 synthesis) [@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_concealing]

The concealing path for AMBE+2 speech frames, as for D-STAR but with 9 correctable raw bits (35 to 43). It dequantizes the received frame and each single-bit flip against copies of the state, makes `Descriptor`s and lets
`Concealer::decide` pick a reading or a (possibly faded, possibly resetting) repeat.

| Argument | Type | Meaning |
|---|---|---|
| `parsed` | `&ParsedFrame` | Parsed frame with the corrected-error counts |

**Returns:** `Option<[i64; N]>`.
**Errors:** none.
**Special concerns:** private; speech frames only.
**Signature:** `fn decode_concealing(&mut self, parsed: &ParsedFrame) -> Option<[i64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `fixed_general_concealment_Concealer_decide`.

### `decode_frame` (float D-STAR synthesis) [@ANCHOR: ham_digital_modes:float_dstar_synthesis_DStarSynthesisDecoder_decode_frame]

The float D-STAR bits-to-PCM decoder for one logical frame, returning 20 ms of `f64` samples. It follows mbelib's bad-frame policy (a speech frame with more than 3 corrected errors reuses the previous parameters
without touching the predictor state, after 3 in a row it mutes and reinitializes), the default concealing path for speech frames, the chip policy's reserved-code handling, and tone synthesis.

| Argument | Type | Meaning |
|---|---|---|
| `logical_frame` | `u128` | 72-bit logical frame |

**Returns:** `Option<[f64; N]>`.
**Errors:** none returned.
**Special concerns:** stateful.
**Signature:** `pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[f64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `fixed_dstar_synthesis_DStarSynthesisDecoder_decode_frame`.

### `decode_concealing` (float D-STAR synthesis) [@ANCHOR: ham_digital_modes:float_dstar_synthesis_DStarSynthesisDecoder_decode_concealing]

Float concealing decoder for D-STAR speech frames: candidate readings from the 11 correctable bits, `Descriptor`s, `Concealer::decide`, then accept or a faded repeat or reset. It is the reference for the fixed one.

| Argument | Type | Meaning |
|---|---|---|
| `parsed` | `&ParsedFrame` | Parsed frame with `epsilon` counts |

**Returns:** `Option<[f64; N]>`.
**Errors:** none.
**Special concerns:** private.
**Signature:** `fn decode_concealing(&mut self, parsed: &ParsedFrame) -> Option<[f64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `float_concealment_Concealer_decide`.

### `decode_frame` (float AMBE+2 synthesis) [@ANCHOR: ham_digital_modes:float_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_frame]

The float AMBE+2 decoder for one logical frame (20 ms of `f64` PCM): parse, classify, concealing path for speech, otherwise the mbelib/chip bad-frame policy, erasure repeat, silence, and tone synthesis for DTMF, single and call-progress tones.

| Argument | Type | Meaning |
|---|---|---|
| `logical_frame` | `u128` | 72-bit logical frame |

**Returns:** `Option<[f64; N]>`.
**Errors:** none returned.
**Special concerns:** stateful.
**Signature:** `pub fn decode_frame(&mut self, logical_frame: u128) -> Option<[f64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `fixed_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_frame`.

### `decode_concealing` (float AMBE+2 synthesis) [@ANCHOR: ham_digital_modes:float_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_concealing]

Float concealing decoder for AMBE+2 speech frames: readings for the received frame and the 9 correctable single-bit flips (35 to 43), `Concealer::decide`, then accept or faded repeat or reset.

| Argument | Type | Meaning |
|---|---|---|
| `parsed` | `&ParsedFrame` | Parsed frame |

**Returns:** `Option<[f64; N]>`.
**Errors:** none.
**Special concerns:** private.
**Signature:** `fn decode_concealing(&mut self, parsed: &ParsedFrame) -> Option<[f64; N]>`
**See also:** test in `tests/ambe_error_policy.rs`; `float_concealment_Concealer_decide`.

### `Concealer::decide` (fixed) [@ANCHOR: ham_digital_modes:fixed_general_concealment_Concealer_decide]

Chooses among the candidate readings of one frame. `candidates[i] = (flips, descriptor)`: the number of raw bits flipped from the received frame (candidate 0 is as received) and the reading's `Descriptor` (`None` if it is not
speech). It updates the bit-error-rate estimate (Q32) from `epsilon / 47`; with no errors and a quiet estimate it accepts candidate 0. Otherwise each candidate costs `flips * ln((1-p)/p)` plus the continuity cost against the previous frame plus
`error_suspicion * epsilon`; the cheapest wins if within the repeat cost, else the previous frame repeats with a fade (`fade_per_frame^repeats`, zero when under 0.03) and a reset after `reset_after` (30) repeats.

| Argument | Type | Meaning |
|---|---|---|
| `candidates` | `&[(u32, Option<Descriptor>)]` | Readings: bit flips and descriptor |
| `epsilon` | `u32` | Errors corrected by the channel code in this frame |

**Returns:** `Decision::Accept(index)` or `Decision::Repeat { scale_q16, reset }`.
**Errors:** none.
**Special concerns:** stateful (previous descriptor, BER, repeat count); all Q16.16 or Q32 integer arithmetic, with `ConcealParams::default()` equal to the float defaults.
**Signature:** `pub fn decide(&mut self, candidates: &[(u32, Option<Descriptor>)], epsilon: u32) -> Decision`
**See also:** test in `tests/ambe_error_policy.rs`; `float_concealment_Concealer_decide`.

### `Descriptor::new` (fixed concealment) [@ANCHOR: ham_digital_modes:fixed_general_concealment_Descriptor_new]

Builds the compact description of a decoded frame that the continuity prior compares: the fundamental `w0_q16`, the overall level in dB (RMS of the harmonic amplitudes, Q16.16), a 16-point spectral shape in dB sampled across
the harmonics, and an 8-band voicing pattern. `voiced` and `ml_q16` are indexed by harmonic starting at 1 (index 0 unused).

| Argument | Type | Meaning |
|---|---|---|
| `w0_q16` | `i32` | Fundamental, Q16.16 |
| `voiced` | `&[bool]` | Voiced flag per harmonic |
| `ml_q16` | `&[i32]` | Amplitude per harmonic, Q16.16 |

**Returns:** `Descriptor`.
**Errors:** divides by the harmonic count; `ml_q16` must have at least two entries (one harmonic) or it panics.
**Special concerns:** pure.
**Signature:** `pub fn new(w0_q16: i32, voiced: &[bool], ml_q16: &[i32]) -> Self`
**See also:** test in `tests/ambe_error_policy.rs`; `fixed_general_concealment_Concealer_decide`.

### `Concealer::decide` (float) [@ANCHOR: ham_digital_modes:float_concealment_Concealer_decide]

The float reference of the concealment decision: `candidates[i] = (flips, descriptor)` where `flips` is the number of raw bits changed from the received frame (candidate 0 is as received, `None` if not a speech frame) and `epsilon` is the
total corrected-error count. It estimates the bit error rate from the 47 protected bits, accepts the received frame when there is no evidence of errors, otherwise picks the cheapest candidate (flip cost, continuity cost, error suspicion) or repeats with a fade.

| Argument | Type | Meaning |
|---|---|---|
| `candidates` | `&[(u32, Option<Descriptor>)]` | Readings |
| `epsilon` | `u32` | Errors corrected in the received frame |

**Returns:** `Decision`.
**Errors:** none.
**Special concerns:** stateful, f64.
**Signature:** `pub fn decide(&mut self, candidates: &[(u32, Option<Descriptor>)], epsilon: u32) -> Decision`
**See also:** test in `tests/ambe_error_policy.rs`; `fixed_general_concealment_Concealer_decide`.

### Entries of this section not yet expanded

The encoders (`Encoder::new`, `next_frame`, `finish` for D-STAR, AMBE+2 and TIA-102, fixed and float, and the TIA analyzer's `finish_input`, `next_analysis`,
`table` and `HighPassFilter::step`) are cited by the real-speech parity tests that run the fixed and float encoders side by side and require identical frame
counts, near-identical parameters and a decoded SNR floor. The synthesis decoders' `decode_frame` and `decode_concealing` (all four) and the concealers'
`decide` (and the fixed `Descriptor::new`) are cited by the policy test that the default decoder fades instead of muting after repeated damaged frames; the TIA
`DecoderState::decode_parameters` and `decode_frame` by the decode tests that match float exactly; `MbeSynthesizer` (`new`, `lift`, `errors_for`,
`synthesize_speech`) by the speech-SNR decline test; the unvoiced and voiced synthesizers (`UnvoicedState::synthesize`, its spectrum, DFT and time-domain
helpers, `unvoiced_dft`, `unvoiced_scaling_coefficient_uncached`, `VoicedState::synthesize`) by the float-parity synthesis tests; and the fixed TIA
`SynthesisState::finalize_parameters` and `synthesize_core` by the float-parity synthesis tests, including the case where the gamma_m gain engages.

  *(Reference: `src/ambe/fixed/tia_102_baba/decode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_decode_DecoderState_decode_parameters]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_decode_DecoderState_decode_frame]`; `src/ambe/fixed/general/mbe_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_synthesize_speech]`, `[@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_lift]`, `[@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_new]`; `src/ambe/float/mbe_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_synthesize_speech]`, `[@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_lift]`, `[@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_errors_for]`; `src/ambe/fixed/general/unvoiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_UnvoicedState_synthesize]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_unvoiced_spectrum_q16]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_unvoiced_dft_q16]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_unvoiced_time_domain_q16]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_time_domain_at_q16]`; `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_tia_102_baba_unvoiced_synthesis_unvoiced_dft]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_unvoiced_synthesis_unvoiced_scaling_coefficient_uncached]`; `src/ambe/fixed/general/voiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_voiced_synthesis_VoicedState_synthesize]`; `src/ambe/fixed/tia_102_baba/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_synthesis_SynthesisState_finalize_parameters]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_synthesis_SynthesisState_synthesize_core]`)*

## Fixed and float AMBE quantizers, reconstruction and wire decode anchored in slice 8

`decode_frame` (RATET(27) wire bytes to the eight confirmed sub-blocks) is cited by the tests on a real captured DTMF frame and a real DTX silence frame. The
shared MBE quantizer (`quantize_speech`, `nearest`, `prev_at`, fixed and float) is cited by the tests that invert the decoder over random indices and that
compare fixed with float on thousands of random frames; `dequantize_speech` and its `prev_at` by the fixed-versus-float dequantize test;
`analyze_at_pitch` (float) and the fixed `analyze_and_quantize`, `frequency_bands_count` and the TIA quantizers (`quantize_gain_vector_q16`,
`quantize_higher_order_coefficients_q16`, `encode_prioritized_bits`, float and fixed) by the real-speech parity tests. The TIA reconstruction chain
(`reconstruct_gain_vector_q16`, `inverse_gain_vector_dct_q16`, `reconstruct_higher_order_coefficients_q16`, `inverse_block_dct_q16`,
`predicted_and_bias_correction_q16`) is cited by the representative-sequence test against the float reconstruction, `weight_q16` by the enhancement test, the
float refiner's `SyntheticSpectrum::at` and `window_dft_16384_direct` by the pitch-refinement test, and `synthesize_repeated_frame_scaled` (fixed and
float) and the fixed `Concealer::continuity_cost_q16` by the concealment policy test.

  *(Reference: `src/ambe/dvsi_p25fec/frame.rs` -> `[@ANCHOR: ham_digital_modes:dvsi_p25fec_frame_decode_frame]`; `src/ambe/fixed/general/concealment.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_concealment_Concealer_continuity_cost_q16]`; `src/ambe/fixed/general/mbe_encode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_mbe_encode_quantize_speech]`, `[@ANCHOR: ham_digital_modes:fixed_general_mbe_encode_nearest]`, `[@ANCHOR: ham_digital_modes:fixed_general_mbe_encode_prev_at]`; `src/ambe/float/mbe_encode.rs` -> `[@ANCHOR: ham_digital_modes:float_mbe_encode_quantize_speech]`, `[@ANCHOR: ham_digital_modes:float_mbe_encode_nearest]`, `[@ANCHOR: ham_digital_modes:float_mbe_encode_prev_at]`, `[@ANCHOR: ham_digital_modes:float_mbe_encode_analyze_at_pitch]`; `src/ambe/fixed/general/mbe_speech.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_mbe_speech_dequantize_speech]`, `[@ANCHOR: ham_digital_modes:fixed_general_mbe_speech_prev_at]`; `src/ambe/fixed/mbe_encode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_mbe_encode_analyze_and_quantize]`; `src/ambe/fixed/tia_102_baba/encode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encode_encode_prioritized_bits]`; `src/ambe/float/tia_102_baba/mod.rs` -> `[@ANCHOR: ham_digital_modes:float_tia_102_baba_mod_encode_prioritized_bits]`; `src/ambe/fixed/tia_102_baba/quantize.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_quantize_quantize_gain_vector_q16]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_quantize_quantize_higher_order_coefficients_q16]`; `src/ambe/fixed/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_reconstruct_reconstruct_gain_vector_q16]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_reconstruct_inverse_gain_vector_dct_q16]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_reconstruct_reconstruct_higher_order_coefficients_q16]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_reconstruct_inverse_block_dct_q16]`; `src/ambe/fixed/tia_102_baba/prediction.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_prediction_predicted_and_bias_correction_q16]`; `src/ambe/fixed/tia_102_baba/enhancement.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_enhancement_weight_q16]`; `src/ambe/fixed/tia_102_baba/vuv.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_frequency_bands_count]`; `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: ham_digital_modes:float_tia_102_baba_pitch_refinement_SyntheticSpectrum_at]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_pitch_refinement_window_dft_16384_direct]`; `src/ambe/fixed/tia_102_baba/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_synthesis_SynthesisState_synthesize_repeated_frame_scaled]`; `src/ambe/float/tia_102_baba/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_tia_102_baba_synthesis_SynthesisState_synthesize_repeated_frame_scaled]`)*

## Codec2 fixed-point internals anchored in slice 9 (LPC, envelope phase, overlap-add, LSP table) and the timing autocorrelation profile

`levinson_durbin_q23_from_integer_r` is the integer-only Levinson-Durbin recursion: for a first-order autocorrelation it returns the exact predictor
`[1, -rho, 0, ...]` in Q8.23 and an all-zero input gives the identity filter. `cheb_poly_eval_q29` evaluates the Chebyshev polynomial in Q2.29 and agrees in sign with
the plain float evaluation over a dense sweep of real captured coefficients. `sample_filter_phase_with` picks, per harmonic, the conjugate of the spectrum bin
nearest `m * wo`; `prepare_subframe_fixed` and `overlap_add_subframe` are the phase/postfilter half and the windowed overlap-add half of one synthesis
sub-frame, tested through the fixed decoder's bit-exact match with the reference decoder. `lsp_cb_q23` is the codec2 1600 LSP codebook start and step values
as exact Q23 shifts. `autocorrelation_profile` is the signed autocorrelation of a feature's first difference over the lags of a baud range: a feature that
flips every symbol shows a negative trough at the symbol rate and a positive peak at two symbols.

  *(Reference: `src/codec2_3200/envelope.rs` -> `[@ANCHOR: ham_digital_modes:codec2_3200_envelope_sample_filter_phase_with]`; `src/codec2_3200/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:codec2_3200_synthesis_SynthesisStateFixed_prepare_subframe_fixed]`, `[@ANCHOR: ham_digital_modes:codec2_3200_synthesis_overlap_add_subframe]`; `src/codec2_3200/lpc.rs` -> `[@ANCHOR: ham_digital_modes:codec2_3200_lpc_cheb_poly_eval_q29]`, `[@ANCHOR: ham_digital_modes:codec2_3200_lpc_levinson_durbin_q23_from_integer_r]`; `src/codec2_1600/lsp_quantiser.rs` -> `[@ANCHOR: ham_digital_modes:codec2_1600_lsp_quantiser_lsp_cb_q23]`; `src/timing_characterizer.rs` -> `[@ANCHOR: ham_digital_modes:timing_characterizer_autocorrelation_profile]`)*

## Float AMBE+2 tone level and unenhanced synthesis anchored in slice 10

The float AMBE+2 encoder's `amplitude_field` maps a tone amplitude to the 12-bit level field by interpolating the chip's seven measured points in log2
amplitude: each measured point is hit exactly, the ends clamp, and the mapping never decreases. `SynthesisState::synthesize_frame_unenhanced` synthesizes from
already-final parameters with no enhancement or smoothing; it is deterministic, rejects a length mismatch, and records its parameters so the next repeated
frame reuses them exactly.

  *(Reference: `src/ambe/float/ambe_plus_2/encoder.rs` -> `[@ANCHOR: ham_digital_modes:float_ambe_plus_2_encoder_amplitude_field]`; `src/ambe/float/tia_102_baba/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_tia_102_baba_synthesis_SynthesisState_synthesize_frame_unenhanced]`)*

## Codec2 1600 test-side reference helpers anchored in slice 11

`FloatBoundaryEncoderFixed::encode` is the codec2 1600 encoder with the float analysis boundary (`analyse_lsps_and_energy_float_boundary` computes the LSPs and
frame energy in float from the integer pitch analysis); the integer encoder is tested to produce the same frames as it. `decode_and_compare_to_reference`
decodes a real captured 1600 bit/s stream and returns the correlation and RMS ratio against the real reference decoder's audio, which two tests require to
be high and near one.
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: ham_digital_modes:codec2_1600_FloatBoundaryEncoderFixed_encode]`, `[@ANCHOR: ham_digital_modes:codec2_1600_analyse_lsps_and_energy_float_boundary]`, `[@ANCHOR: ham_digital_modes:codec2_1600_decode_and_compare_to_reference]`)*

## Codec2 3200 synthesis (float and fixed)

`synthesize_subframe` turns one decoded model into 10 ms of audio: it samples the LPC filter's phase
response, sets harmonic phases (`synthesize_phase`: voiced harmonics follow one tracked fundamental
phase, unvoiced ones get random phase), applies the `postfilter` (randomises the phase of harmonics that
are quiet relative to the tracked background level during voiced frames, and tracks that level from
unvoiced frames), inverse-transforms, and overlap-adds. `ear_protection` attenuates a whole frame by the
square of the overshoot when any sample exceeds 30000, guarding against bit-error spikes. The
`*_fixed` twins (`synthesize_subframe_fixed`, `synthesize_phase_fixed`, `postfilter_fixed`,
`ear_protection_fixed`) do the same in integer Q23 arithmetic; `ear_protection_fixed` uses one exact
integer division for the gain rather than a lookup-table round trip.
*(Reference: `src/codec2_3200/synthesis.rs` -> `[@ANCHOR: SynthesisState::synthesize_subframe]`, `[@ANCHOR: ham_digital_modes:synthesize_phase]`, `[@ANCHOR: ham_digital_modes:postfilter]`, `[@ANCHOR: ham_digital_modes:ear_protection]`, `[@ANCHOR: SynthesisStateFixed::synthesize_subframe_fixed]`, `[@ANCHOR: ham_digital_modes:synthesize_phase_fixed]`, `[@ANCHOR: ham_digital_modes:postfilter_fixed]`, `[@ANCHOR: ham_digital_modes:ear_protection_fixed]`)*

`fft_fixed` is the in-place radix-2 decimation-in-time FFT in Q23 integers used by the fixed synthesis, at
512 points and, for the spectral bridge, 1024. Sizes must be a power of two with a cached bit-reversal
table (anything else panics); `forward` selects the transform direction; there is no per-stage rescaling.
*(Reference: `src/codec2_3200/fixed_fft.rs` -> `[@ANCHOR: ham_digital_modes:fft_fixed]`)*

## Codec2 3200 spectral bridge

The spectral bridge synthesizes a higher-sample-rate frame from an already-decoded 8 kHz model.
`make_synthesis_window_sb` builds its overlap-add synthesis window at the doubled frame size, and
`synthesize_subframe_sb` synthesizes one 10 ms sub-frame at the higher rate from the unchanged base model,
using extrapolated amplitudes and its own tracked phase.
*(Reference: `src/codec2_3200/spectral_bridge.rs` -> `[@ANCHOR: ham_digital_modes:make_synthesis_window_sb]`, `[@ANCHOR: SpectralBridgeState::synthesize_subframe_sb]`)*

## Codec2 3200 integer encoder boundaries

The 3200 bit-per-second encoder (`encoder_fixed.rs`) runs from samples to transmitted indices in integer
arithmetic with no floating point; the float entry points (`encode_energy`, `nlp_fixed`, `lpc_energy_fixed`,
`encode_lsps_delta_scalar_fixed`) remain for the other codec modes and the tests, and delegate to the
integer cores below.

- `nlp_fixed_bin` is the pitch estimator proper. It squares the new samples, notches out DC, decimates,
  applies a Hann window, transforms, and searches the power spectrum for the strongest bin between the
  minimum and maximum pitch. It returns the winning FFT bin (fundamental = `bin * 3.125` Hz) rather than a
  frequency, then checks sub-multiples of the peak. Only the 129 bins that can be read are computed.
  *(Reference: `src/codec2_3200/nlp.rs` -> `[@ANCHOR: nlp_fixed_bin]`)*
- `fft_fixed_sparse_prefix` is the same transform as `fft_fixed` for an input whose only nonzero entries are
  the first few real samples (`re[i] = input[i]`, `im` zero). The output is bit-identical, but the input is
  scattered straight to bit-reversed positions and butterfly blocks that only see known zeros are skipped or
  reduced to copies, so neither array needs clearing first.
  *(Reference: `src/codec2_3200/fixed_fft.rs` -> `[@ANCHOR: fft_fixed_sparse_prefix]`)*
- `lpc_energy_q23` is the frame energy, the sum of `a[i] * r[i]` over the linear prediction coefficients and
  autocorrelation, in Q23 with each product truncated by a right shift back to Q23 before summing.
  *(Reference: `src/codec2_3200/lpc.rs` -> `[@ANCHOR: lpc_energy_q23]`)*
- `encode_energy_q23` maps that linear energy to the transmitted energy index: `10 log10` via an integer
  base-2 logarithm, then the same uniform `E_BITS`-level quantiser over the decibel range as `encode_energy`,
  round to nearest and clamped. It can differ from the float quantiser only for energies within about 1e-5 dB
  of a quantiser boundary.
  *(Reference: `src/codec2_3200/quantise.rs` -> `[@ANCHOR: encode_energy_q23]`)*
- `acos_lut_q23` returns `acos(x)` for `x` in Q23 (clamped to `[-1, 1]`) as Q23 radians, by a table lookup
  with linear interpolation on `|x|`, reflecting negative `x` as `pi - acos(|x|)`.
  *(Reference: `src/codec2_3200/lpc.rs` -> `[@ANCHOR: acos_lut_q23]`)*
- `find_next_root_q29` finds the next root of one of the two Chebyshev-form line spectral polynomials: a
  0.01-step grid search downward from a start value in Q29, then six bisections once the sign changes. It
  returns `None` when no root remains. `lpc_to_lsp_q23` runs it alternately on the two polynomials, converts
  each root to Q23, and takes `acos_lut_q23` of it, giving the ten line spectral pair angles in Q23
  radians, or `None` if any root is not found.
  *(Reference: `src/codec2_3200/lpc.rs` -> `[@ANCHOR: find_next_root_q29]`, `[@ANCHOR: lpc_to_lsp_q23]`)*
- `encode_lsps_delta_scalar_q23` quantises those angles: each is converted to Hz in Q16, differenced against
  the previous quantised value (the first is not), snapped to the nearest level of that dimension's table, and
  the quantised value is accumulated for the next difference.
  *(Reference: `src/codec2_3200/quantise.rs` -> `[@ANCHOR: encode_lsps_delta_scalar_q23]`)*
