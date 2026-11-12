# Story: Codec2 3200 and AMBE Encoder Internals (ham_digital_modes)

These are the low-level building blocks of the Codec2 3200 encoder/decoder (float reference and the
integer-only "fixed" port) and of the TIA-102.BABA AMBE wire layer. Each entry says what the piece does;
the tests that anchor to them compare the fixed port against the float reference or against a captured
real bitstream.

## AMBE wire layer

`encode_code_vectors` takes the eight prioritized bit vectors, applies Golay (first four) and Hamming
(next three) forward error correction, leaves the last vector uncoded, and then applies the standard's
pseudo-random bit modulation (Eq. 84-94). It trusts the caller for each vector's bit width. The DVSI chip's
own framing lives in the separate `*_chip` variants.
*(Reference: `src/ambe/float/tia_102_baba/mod.rs` -> `[@ANCHOR: ambe_mod:encode_code_vectors]`)*

## Codec2 1600 fixed-point LSP quantiser

`quantise_dim_fixed` finds the nearest evenly-spaced quantiser level for a target frequency in Q23 integer
arithmetic: it rounds `(target - start) / step` to the nearest integer with integer division, and clamps
the index into `0..levels`. A target at or below the start maps to level 0.
*(Reference: `src/codec2_1600/lsp_quantiser.rs` -> `[@ANCHOR: ham_digital_modes:quantise_dim_fixed]`)*

## Codec2 3200 model and analysis

- `Model::new` builds the harmonic model for a fundamental `wo` and voicing flag: the harmonic count is
  `PI/wo` capped at the maximum, amplitudes start at zero and phases at unit real. `ModelFixed::new` is the
  Q23 twin; a zero `wo` is treated as one so both saturate to the same maximum count.
  *(Reference: `src/codec2_3200/envelope.rs` -> `[@ANCHOR: Model::new]`, `[@ANCHOR: ModelFixed::new]`)*
- `Encoder::shift_in` (float reference encoder) slides the pitch-analysis history window left by one frame
  and appends the new 16-bit samples as floats.
  *(Reference: `src/codec2_3200/floating_reference/mod.rs` -> `[@ANCHOR: Encoder::shift_in]`)*
- `lsp_dim_value_hz` converts a quantiser level for one LSP dimension into its frequency in Hz: uniform
  steps up to the dimension's breakpoint level and a second step size beyond it (the widened dimensions).
  *(Reference: `src/codec2_3200/quantise.rs` -> `[@ANCHOR: ham_digital_modes:lsp_dim_value_hz]`)*
- `div_round_i64` (voicing) and `div_round_i128` (LPC recursion) divide with round-to-nearest, half away
  from zero, for a positive divisor; the divisor is `debug_assert`ed positive.
  *(Reference: `src/codec2_3200/voicing.rs` -> `[@ANCHOR: ham_digital_modes:div_round_i64]`; `src/codec2_3200/lpc.rs` -> `[@ANCHOR: ham_digital_modes:div_round_i128]`)*

## Codec2 3200 pitch estimator (nlp)

- `design_lowpass` builds the 25-tap Hann-windowed sinc anti-alias filter used before decimating for pitch
  estimation, normalised to unity DC gain.
- `dc_notch_fixed` is the fixed-point first-order DC notch on squared samples; only the notch coefficient
  is Q23.
- `rshift_round_i128` (anchored `nlp:`) is a rounding right shift that narrows to `i64` and debug-asserts
  the result fits; `rshift_round_i128_wide` keeps the full `i128` width for the sub-multiple threshold
  (`CNLP * gmax`), which can exceed `i64::MAX` at full-scale input and once silently wrapped.
*(Reference: `src/codec2_3200/nlp.rs` -> `[@ANCHOR: ham_digital_modes:design_lowpass]`, `[@ANCHOR: ham_digital_modes:dc_notch_fixed]`, `[@ANCHOR: nlp:rshift_round_i128]`, `[@ANCHOR: ham_digital_modes:rshift_round_i128_wide]`)*

## Codec2 3200 fixed-point FFT arithmetic helpers (fixed_fft)

The integer FFT keeps every butterfly exact while avoiding 128-bit arithmetic whenever the values are small enough, so a few
small helpers choose between equivalent forms; each is tested against the plain exact `i128` expression.

- `ComplexQ23::mag_sq_raw` is the squared magnitude `re^2 + im^2` in Q46, computed in `u64` when both parts are under 2^31 and in
  `i128` otherwise; the two paths give the same number.
- `twiddle_mul` multiplies a Q23 twiddle by a complex value and rounds back to Q23: in `i64` when both operands are under 2^38,
  otherwise in exact `i128`; the result is identical either way.
- `mode_for_sum` picks the kernel for a transform from the sum of its input magnitudes: the 32-bit mode below 2^29, the 64-bit kernel
  of the target below 2^39, and the checked mode above that.
- `mul_w_x`, `umul_x` and `mul_round_i64` do 24-bit-by-64-bit multiplies as 32-bit halves, for targets without a 64-bit multiplier;
  they equal the plain 64-bit product (and, for `mul_round_i64`, its Q23-rounded two-term sum).
- `twiddle_sums` is the pair of exact 64-bit sums `c*x - s*y` and `c*y + s*x` of an inverse-transform twiddle multiply, as 32-bit
  word pairs; `round_shift` and `round_shift_neg` turn such a pair into the Q23-rounded value of the sum and of its negation.
  *(Reference: `src/codec2_3200/fixed_fft.rs` -> `[@ANCHOR: fixed_fft:ComplexQ23_mag_sq_raw]`, `[@ANCHOR: fixed_fft:twiddle_mul]`, `[@ANCHOR: fixed_fft:mode_for_sum]`, `[@ANCHOR: fixed_fft:mul_w_x]`, `[@ANCHOR: fixed_fft:umul_x]`, `[@ANCHOR: fixed_fft:mul_round_i64]`, `[@ANCHOR: fixed_fft:twiddle_sums]`, `[@ANCHOR: fixed_fft:round_shift]`, `[@ANCHOR: fixed_fft:round_shift_neg]`)*

## Codec2 3200 small integer helpers (fixed_point, nlp, envelope, synthesis)

- `clz_nonzero` counts the leading zeros of a nonzero `u32` with two range steps and a 256-byte table, because a core without a
  count-leading-zeros instruction expands `leading_zeros` into a long sequence and `log2_q23` runs it about a thousand times per frame;
  it must equal `leading_zeros` for every nonzero input.
- `table_is_non_decreasing` is the `const fn` that proves at compile time that the log2 and exp2 interpolation tables never step down,
  which the interpolation arithmetic relies on.
- `window_fits_i32` and `window_is_bounded` are the `const fn` checks that a window table fits `i32` (so a 32-bit kernel may use it) and
  stays within +-1.0 in Q23 (so a product with it cannot overflow).
- `square_words` squares a 64-bit magnitude into two 64-bit words from three 32-bit products, `power_bin` is `re^2 + im^2` as an exact
  `i128` (dispatching to the native 128-bit form or the 32-bit-limb form by target), and `mag_sq_q23` is the squared magnitude of an
  LPC-spectrum bin rounded to Q23 on whichever of three paths fits the operands.
- `mul_q23` multiplies a transform output by a Q23 window value, in `i64` unless the output is large enough that the product could
  overflow, in which case it widens to `i128`.
  *(Reference: `src/codec2_3200/fixed_point.rs` -> `[@ANCHOR: ham_digital_modes:clz_nonzero]`, `[@ANCHOR: ham_digital_modes:table_is_non_decreasing]`; `src/codec2_3200/nlp.rs` -> `[@ANCHOR: ham_digital_modes:window_fits_i32]`, `[@ANCHOR: ham_digital_modes:square_words]`, `[@ANCHOR: ham_digital_modes:power_bin]`; `src/codec2_3200/envelope.rs` -> `[@ANCHOR: ham_digital_modes:mag_sq_q23]`; `src/codec2_3200/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:mul_q23]`, `[@ANCHOR: ham_digital_modes:window_is_bounded]`)*

## Small helpers anchored in slice 3 (wire layout, FEC, tables, tracking, header packing, timing)

- `natural_range` (P25 chip wire format) gives the `(start, length)` of each natural-order block: four 23-bit Golay blocks, three 15-bit
  Hamming blocks and the 7 raw bits tile the 144-bit frame exactly once, and an index past the last block of its kind is refused.
- `hamming_decode_chip` is the minimum-distance decoder of the chip's Hamming(15,11) code; it recovers every data word, corrects any one
  flipped bit, and ignores bit 15.
- `gain_vector_bits` is the integer-only `bits` half of the gain bit-allocation table, `None` outside L 9..=56 and M 2..=6.
- `tracking_range` (fixed-point pitch tracking) is the contiguous run of candidate indices within 0.8 to 1.2 of the reference pitch,
  evaluated in exact half samples.
- `ratio_q30` divides two wide Q60 energies into a Q30 ratio, shifting both down first so the numerator cannot overflow.
- `DStarHeader::pack_field` writes one callsign field space-padded to its width, refusing non-ASCII text and an overlong value without
  appending anything.
- `two_tone_window_len` picks the midpoint of the valid window band for the two-tone timing feature, or `None` when no window satisfies
  both constraints.
  *(Reference: `src/ambe/dvsi_p25fec/wire_format.rs` -> `[@ANCHOR: ham_digital_modes:natural_range]`; `src/ambe/dvsi_p25fec/fec.rs` -> `[@ANCHOR: ham_digital_modes:hamming_decode_chip]`; `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: ham_digital_modes:gain_vector_bits]`; `src/ambe/fixed/tia_102_baba/pitch.rs` -> `[@ANCHOR: ham_digital_modes:tracking_range]`; `src/ambe/fixed/tia_102_baba/vuv.rs` -> `[@ANCHOR: ham_digital_modes:ratio_q30]`; `src/dstar.rs` -> `[@ANCHOR: ham_digital_modes:DStarHeader_pack_field]`; `src/timing_characterizer.rs` -> `[@ANCHOR: ham_digital_modes:two_tone_window_len]`)*

## Fixed-point AMBE helpers anchored in slice 4 (noise, twiddles, dequantizer, blocks)

- `NoiseState` (`new`, `step`, `advance_frame`, `at`) is the integer noise generator of Eq. 117, `u(n+1) = (171 u(n) + 11213) mod 53125`
  from `u(-105) = 3147`: `new` runs it 209 steps, `at` reads `u(-104..=104)` relative to the current frame and refuses anything outside,
  and `advance_frame` moves the window on by one 160-sample frame.
- `ComplexQ16::norm_sqr_q16` is the squared magnitude of an `i64` Q16.16 spectral value through an `i128` intermediate, so a large bin
  cannot overflow; `mul_div_wide` is `a * b / c` in one widened division (zero for a zero divisor), used to rescale a bin without ever
  forming the tiny scale factor on its own.
- `twiddle_q16` is `(cos, sin)` of `2 pi m n / 256` from a table of the 256 distinct phases, so the 256-point transforms evaluate no
  trigonometry per term.
- `phase_dither_q16` maps the noise sample of harmonic `l` linearly onto `[-pi, pi)` (Eq. 141) in Q16.16 radians.
- `dequantize_uniform_q16` is the shared bin-centre formula of Eq. 68 and 71, `step * (value - 2^(bits-1) + 0.5)`, zero for zero bits;
  `previous_log2_amplitude_q16` is `log2` of the previous frame's harmonic amplitude, zero for `j = 0` and clamped to the last harmonic;
  `partition_into_blocks_q16` splits the `L` prediction residuals into Annex J's six blocks.
  *(Reference: `src/ambe/fixed/general/unvoiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:NoiseState_new]`, `[@ANCHOR: ham_digital_modes:NoiseState_step]`, `[@ANCHOR: ham_digital_modes:NoiseState_advance_frame]`, `[@ANCHOR: ham_digital_modes:NoiseState_at]`, `[@ANCHOR: ham_digital_modes:ComplexQ16_norm_sqr_q16]`, `[@ANCHOR: ham_digital_modes:mul_div_wide]`, `[@ANCHOR: ham_digital_modes:twiddle_q16]`; `src/ambe/fixed/general/voiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:phase_dither_q16]`; `src/ambe/fixed/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: ham_digital_modes:dequantize_uniform_q16]`; `src/ambe/fixed/tia_102_baba/prediction.rs` -> `[@ANCHOR: ham_digital_modes:previous_log2_amplitude_q16]`; `src/ambe/fixed/tia_102_baba/quantize.rs` -> `[@ANCHOR: ham_digital_modes:partition_into_blocks_q16]`)*

## Damaged-frame handling in the float AMBE decoder (slice 5)

- `ErrorPolicy::is_bad` decides whether a speech frame counts as damaged: more than three corrected errors in total under the clean and
  concealing policies, three or more in the first code word under the chip-compatible policy, which ignores the second.
- `ErrorPolicy::bad_frame_action` answers what to do with the n-th damaged frame in a row: repeat the previous frame for the first three
  under every policy, then mute (clean, concealing) or decode the damaged frame anyway (chip-compatible).
- `Descriptor::new` condenses a decoded frame to what the concealer compares between frames: the pitch, the level in dB, the dB amplitude
  at sixteen evenly spaced harmonics (floored at -60 dB) and the voicing of eight bands.
- `Concealer::continuity_cost` is the cost in nats of accepting a candidate frame given the previous accepted one: a level term, a spectral
  shape term, a cost per voicing band that differs, and a pitch-ratio term when both frames are voiced; the level, shape and pitch priors
  widen with the number of frames repeated in a row, the voicing cost does not.
  *(Reference: `src/ambe/float/mbe_synthesis.rs` -> `[@ANCHOR: ham_digital_modes_mbe:ErrorPolicy_is_bad]`, `[@ANCHOR: ham_digital_modes_mbe:ErrorPolicy_bad_frame_action]`; `src/ambe/float/concealment.rs` -> `[@ANCHOR: ham_digital_modes_float_conceal:Descriptor_new]`, `[@ANCHOR: ham_digital_modes_float_conceal:Concealer_continuity_cost]`)*

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

Each function here already had an exact-value or float-parity test; the tests now cite them. `g3_encode` and `hamming_encode_chip` are the chip's
own `g3` and 15-bit Hamming encoders (zero encodes to zero, every codeword round-trips, the weight distribution matches the known enumerator).
The fixed `dequantize` for AMBE+2 and D-STAR gives the same parameters as the float one over representative frame sequences, and the fixed
`quantize_pitch_p8` equals the float `quantize_pitch` for every pitch period. `classify_tone_index` (fixed and float D-STAR) maps a tone index to
the same frequency, `classify_b0` separates tone-bearing sub-codes from erasures, `frame_to_wire_bytes` and `wire_bytes_to_frame` are the exact
inverses of each other over the D-STAR interleave, `build_frame` packs 49 bits into a frame that survives the wire round trip, and `update_xi_max`
follows Eq. 41 in all three branches.

  *(Reference: `src/ambe/dvsi_p25fec/fec.rs` -> `[@ANCHOR: ham_digital_modes:dvsi_p25fec_fec_g3_encode]`, `[@ANCHOR: ham_digital_modes:dvsi_p25fec_fec_hamming_encode_chip]`; `src/ambe/fixed/ambe_plus_2/decode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_decode_dequantize]`; `src/ambe/fixed/dstar/decode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_dstar_decode_dequantize]`, `[@ANCHOR: ham_digital_modes:fixed_dstar_decode_classify_tone_index]`; `src/ambe/fixed/ambe_plus_2/encode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_encode_quantize_pitch_p8]`; `src/ambe/fixed/dstar/encode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_dstar_encode_quantize_pitch_p8]`; `src/ambe/float/dstar/decode.rs` -> `[@ANCHOR: ham_digital_modes:float_dstar_decode_classify_tone_index]`, `[@ANCHOR: ham_digital_modes:float_dstar_decode_classify_b0]`; `src/ambe/float/ambe_plus_2/decode.rs` -> `[@ANCHOR: ham_digital_modes:float_ambe_plus_2_decode_classify_b0]`; `src/ambe/float/dstar/quantize.rs` -> `[@ANCHOR: ham_digital_modes:float_dstar_quantize_quantize_pitch]`; `src/ambe/float/dstar/interleave.rs` -> `[@ANCHOR: ham_digital_modes:float_dstar_interleave_frame_to_wire_bytes]`, `[@ANCHOR: ham_digital_modes:float_dstar_interleave_wire_bytes_to_frame]`; `src/ambe/float/dstar/encode.rs` -> `[@ANCHOR: ham_digital_modes:float_dstar_encode_build_frame]`; `src/ambe/fixed/tia_102_baba/vuv.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_update_xi_max]`)*

## Fixed-point TIA-102 analysis chain anchored in slice 6 (pitch tracking and refinement, voicing, amplitudes)

The encoder's analysis side is tested end to end against its float sibling on real speech, and each step is cited by those tests.
`PitchAnalysisFrame` (`new`, `error_function`, `error_table`) builds the pitch error table E(P) for the 102 candidate periods (a silent frame has unit
error and the default history index); `look_back_pitch_tracking`, `look_ahead_pitch_tracking` and `choose_initial_pitch_estimate` pick the initial
estimate from it. `RefinementFrame` (`new`, `sw_at`), `window_dft_16384_q22`, `harmonic_amplitude_q16`, `spectrum_error_and_energy`,
`refinement_error` and `refine_pitch` refine that estimate to an eighth of a sample, agreeing with the float refiner to the same P8 value on nearly every
frame. `xi_over`, `voicing_threshold_q30`, `voicing_measure_q30` and `determine_voicing` make the per-band voiced/unvoiced decisions (Eq. 35 to 41) and
`estimate_spectral_amplitudes_q16`, `voiced_amplitude_q16` and `unvoiced_amplitude_q16` the harmonic amplitudes; `analyze_at_pitch` ties them together for
one frame. Over the real-speech frames the voicing decisions agree with float on all but a handful of harmonics and the amplitudes to better than
0.001 dB where voicing agrees.

  *(Reference: `src/ambe/fixed/tia_102_baba/pitch.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_PitchAnalysisFrame_new]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_PitchAnalysisFrame_error_function]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_PitchAnalysisFrame_error_table]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_choose_initial_pitch_estimate]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_look_ahead_pitch_tracking]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_look_back_pitch_tracking]`; `src/ambe/fixed/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_RefinementFrame_new]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_RefinementFrame_sw_at]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_window_dft_16384_q22]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_harmonic_amplitude_q16]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_spectrum_error_and_energy]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_refine_pitch]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_pitch_refinement_refinement_error]`; `src/ambe/fixed/tia_102_baba/vuv.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_determine_voicing]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_voicing_measure_q30]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_voicing_threshold_q30]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_vuv_xi_over]`; `src/ambe/fixed/tia_102_baba/spectral_amplitude.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_spectral_amplitude_estimate_spectral_amplitudes_q16]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_spectral_amplitude_voiced_amplitude_q16]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_spectral_amplitude_unvoiced_amplitude_q16]`; `src/ambe/fixed/mbe_encode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_mbe_encode_analyze_at_pitch]`)*

## Fixed and float AMBE codec chain anchored in slice 7 (encoders, decoders, synthesis, concealment)

The encoders (`Encoder::new`, `next_frame`, `finish` for D-STAR, AMBE+2 and TIA-102, fixed and float, and the TIA analyzer's `finish_input`, `next_analysis`,
`table` and `HighPassFilter::step`) are cited by the real-speech parity tests that run the fixed and float encoders side by side and require identical frame
counts, near-identical parameters and a decoded SNR floor. The synthesis decoders' `decode_frame` and `decode_concealing` (all four) and the concealers'
`decide` (and the fixed `Descriptor::new`) are cited by the policy test that the default decoder fades instead of muting after repeated damaged frames; the TIA
`DecoderState::decode_parameters` and `decode_frame` by the decode tests that match float exactly; `MbeSynthesizer` (`new`, `lift`, `errors_for`,
`synthesize_speech`) by the speech-SNR decline test; the unvoiced and voiced synthesizers (`UnvoicedState::synthesize`, its spectrum, DFT and time-domain
helpers, `unvoiced_dft`, `unvoiced_scaling_coefficient_uncached`, `VoicedState::synthesize`) by the float-parity synthesis tests; and the fixed TIA
`SynthesisState::finalize_parameters` and `synthesize_core` by the float-parity synthesis tests, including the case where the gamma_m gain engages.

  *(Reference: `src/ambe/fixed/dstar/encoder.rs` -> `[@ANCHOR: ham_digital_modes:fixed_dstar_encoder_Encoder_next_frame]`, `[@ANCHOR: ham_digital_modes:fixed_dstar_encoder_Encoder_finish]`, `[@ANCHOR: ham_digital_modes:fixed_dstar_encoder_Encoder_new]`; `src/ambe/fixed/ambe_plus_2/encoder.rs` -> `[@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_encoder_Encoder_next_frame]`, `[@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_encoder_Encoder_finish]`; `src/ambe/fixed/tia_102_baba/encoder.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_Encoder_next_frame]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_Encoder_finish]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_FrameAnalyzer_finish_input]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_FrameAnalyzer_next_analysis]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_FrameAnalyzer_table]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_encoder_HighPassFilter_step]`; `src/ambe/float/dstar/encoder.rs` -> `[@ANCHOR: ham_digital_modes:float_dstar_encoder_Encoder_next_frame]`, `[@ANCHOR: ham_digital_modes:float_dstar_encoder_Encoder_finish]`, `[@ANCHOR: ham_digital_modes:float_dstar_encoder_Encoder_new]`; `src/ambe/float/ambe_plus_2/encoder.rs` -> `[@ANCHOR: ham_digital_modes:float_ambe_plus_2_encoder_Encoder_next_frame]`, `[@ANCHOR: ham_digital_modes:float_ambe_plus_2_encoder_Encoder_finish]`; `src/ambe/float/tia_102_baba/encoder.rs` -> `[@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_Encoder_next_frame]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_Encoder_finish]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_FrameAnalyzer_finish_input]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_FrameAnalyzer_next_analysis]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_FrameAnalyzer_table]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_encoder_HighPassFilter_step]`; `src/ambe/fixed/dstar/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_dstar_synthesis_DStarSynthesisDecoder_decode_frame]`, `[@ANCHOR: ham_digital_modes:fixed_dstar_synthesis_DStarSynthesisDecoder_decode_concealing]`; `src/ambe/fixed/ambe_plus_2/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_frame]`, `[@ANCHOR: ham_digital_modes:fixed_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_concealing]`; `src/ambe/float/dstar/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_dstar_synthesis_DStarSynthesisDecoder_decode_frame]`, `[@ANCHOR: ham_digital_modes:float_dstar_synthesis_DStarSynthesisDecoder_decode_concealing]`; `src/ambe/float/ambe_plus_2/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_frame]`, `[@ANCHOR: ham_digital_modes:float_ambe_plus_2_synthesis_AmbePlus2SynthesisDecoder_decode_concealing]`; `src/ambe/fixed/general/concealment.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_concealment_Concealer_decide]`, `[@ANCHOR: ham_digital_modes:fixed_general_concealment_Descriptor_new]`; `src/ambe/float/concealment.rs` -> `[@ANCHOR: ham_digital_modes:float_concealment_Concealer_decide]`; `src/ambe/fixed/tia_102_baba/decode.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_decode_DecoderState_decode_parameters]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_decode_DecoderState_decode_frame]`; `src/ambe/fixed/general/mbe_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_synthesize_speech]`, `[@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_lift]`, `[@ANCHOR: ham_digital_modes:fixed_general_mbe_synthesis_MbeSynthesizer_new]`; `src/ambe/float/mbe_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_synthesize_speech]`, `[@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_lift]`, `[@ANCHOR: ham_digital_modes:float_mbe_synthesis_MbeSynthesizer_errors_for]`; `src/ambe/fixed/general/unvoiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_UnvoicedState_synthesize]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_unvoiced_spectrum_q16]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_unvoiced_dft_q16]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_unvoiced_time_domain_q16]`, `[@ANCHOR: ham_digital_modes:fixed_general_unvoiced_synthesis_time_domain_at_q16]`; `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:float_tia_102_baba_unvoiced_synthesis_unvoiced_dft]`, `[@ANCHOR: ham_digital_modes:float_tia_102_baba_unvoiced_synthesis_unvoiced_scaling_coefficient_uncached]`; `src/ambe/fixed/general/voiced_synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_general_voiced_synthesis_VoicedState_synthesize]`; `src/ambe/fixed/tia_102_baba/synthesis.rs` -> `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_synthesis_SynthesisState_finalize_parameters]`, `[@ANCHOR: ham_digital_modes:fixed_tia_102_baba_synthesis_SynthesisState_synthesize_core]`)*

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
