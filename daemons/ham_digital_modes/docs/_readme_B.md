## Codec2 1600 decoder LSP post-processing

The 1600 bps mode transmits one set of line spectral pairs (LSPs) per 40 ms, so after the scalar indexes are
dequantised the decoder repairs and spreads them, then interpolates them across the four 10 ms sub-frames.
All of these live in `src/codec2_1600/lsp_post.rs`; each has a float form and a Q23 integer twin that is
checked against it.

- `check_lsp_order` forces ascending order after dequantisation: whenever an adjacent pair is out of order it
  swaps them and pushes the pair apart by 0.1 rad (the lower slot becomes `old_upper - 0.1`, the upper slot
  `old_lower + 0.1`), and returns the number of swaps. The scan restarts after a swap in the same quirky way
  the reference does (the restart lands on index 2, not 1), reproduced on purpose rather than "fixed".
  *(Reference: `[@ANCHOR: check_lsp_order]`)*
- `check_lsp_order_fixed` is the same logic on `i64` Q23 angles, with the 0.1 rad nudge as the constant
  838861.
  *(Reference: `[@ANCHOR: check_lsp_order_fixed]`)*
- `bw_expand_lsps` enforces a minimum gap between neighbouring LSPs in one upward pass: gaps at indexes 1 to 3
  are widened to `min_sep_low` Hz and gaps at indexes 4 to 9 to `min_sep_high` Hz (the decoder passes 50 and
  100), converting Hz to radians with `pi/4000`.
  *(Reference: `[@ANCHOR: bw_expand_lsps]`)*
- `bw_expand_lsps_fixed` is the Q23 twin; the margins arrive already in Q23 (`min_sep_low_q23()` and
  `min_sep_high_q23()`, 329420 and 658840).
  *(Reference: `[@ANCHOR: bw_expand_lsps_fixed]`)*
- `interpolate_lsp_ver2` blends the previous frame's last LSPs with the new ones element by element,
  `(1 - w) * prev + w * next`; the decoder uses weights 0.25, 0.5 and 0.75 for sub-frames 0 to 2 (the 3200
  decoder's `interpolate_lsp` is the fixed 0.5 case).
  *(Reference: `[@ANCHOR: interpolate_lsp_ver2]`)*
- `interpolate_lsp_ver2_fixed` restricts the weight to `quarters` of 1, 2 or 3 (debug-asserted), so the blend
  is `((4 - q) * prev + q * next + 2) >> 2`: exact integer arithmetic with a single rounding.
  *(Reference: `[@ANCHOR: interpolate_lsp_ver2_fixed]`)*

## Codec2 1600 scalar LSP quantiser

`src/codec2_1600/lsp_quantiser.rs`. The ten LSP dimensions each have an evenly spaced codebook
(`start_hz + step_hz * index`), so quantising is a rounded division, not a table search.

- `lsp_bits` gives the bit width of dimension `i`: 4 for dimensions 0 to 6, 3 for 7 and 8, 2 for 9
  (36 bits total); the frame packer uses it for the LSP field widths.
  *(Reference: `[@ANCHOR: lsp_bits]`)*
- `quantise_dim` (float) maps one target frequency in Hz to the nearest level of one dimension: round
  `(target - start) / step` and clamp into `0..levels`.
  *(Reference: `[@ANCHOR: quantise_dim]`)*
- `encode_lsps_scalar` converts ten LSPs from radians to Hz (`4000/pi`) and quantises each dimension
  independently with `quantise_dim`.
  *(Reference: `[@ANCHOR: encode_lsps_scalar]`)*
- `decode_lsps_scalar` is the inverse: `start + step * index` in Hz, converted back to radians. This is the
  table any compliant decoder must reproduce.
  *(Reference: `[@ANCHOR: decode_lsps_scalar]`)*
- `q_mul_q23` multiplies two Q23 numbers in `i128` and shifts the product back to Q23 (rounding toward minus infinity); the
  fixed LSP encoder and decoder use it for the radian/Hz conversions.
  *(Reference: `[@ANCHOR: q_mul_q23]`)*
- `encode_lsps_scalar_fixed` is the integer encoder: each Q23 radian angle goes to Hz with `q_mul_q23` and
  `4000/pi`, then through the integer nearest-level search (`quantise_dim_fixed`).
  *(Reference: `[@ANCHOR: encode_lsps_scalar_fixed]`)*
- `decode_lsps_scalar_fixed` computes `start + step * index` in Q23 Hz and converts to Q23 radians with
  `q_mul_q23` and `pi/4000`.
  *(Reference: `[@ANCHOR: decode_lsps_scalar_fixed]`)*

## Codec2 1600 encoder internals

- `analyse_lsps_and_energy` (float) is one linear-prediction pass over the 320-sample history window: apply
  the analysis window, autocorrelate, apply the white-noise correction, run Levinson-Durbin, measure the
  prediction energy on the uncorrected autocorrelation, expand bandwidth with `bw_gamma`, and convert to LSPs
  (evenly spaced fallback if the root search fails). The encoder runs it twice per frame but sends the LSPs
  of the second pass only; the first pass exists for its energy.
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: analyse_lsps_and_energy]`)*
- `analyse_lsps_and_energy_fixed` (the function `analyse_energy_and_ak_fixed`) is the integer version of the
  same pass: Q30 window times samples (shifted to Q8.23), integer autocorrelation, integer white-noise
  correction and Levinson-Durbin, energy before bandwidth expansion, then `apply_bw_gamma_fixed`. It returns
  the Q23 energy and the expanded coefficients; the LSP root search is a separate step.
  *(Reference: `[@ANCHOR: analyse_lsps_and_energy_fixed]`)*
- `codec2_1600:Encoder::shift_in` slides the float history window left by 80 samples and appends the next
  80 new samples as floats; it asserts the slice is exactly 80 samples long instead of silently truncating.
  *(Reference: `[@ANCHOR: codec2_1600:Encoder::shift_in]`)*
- `codec2_1600:EncoderFixed::shift_in` does the same on the `i16` history, failing on a length mismatch
  through `copy_from_slice`.
  *(Reference: `[@ANCHOR: codec2_1600:EncoderFixed::shift_in]`)*

## Bit packing primitives (3200 and 1600)

`src/codec2_3200/bits.rs`, shared by both frame formats.

- `gray_to_binary` undoes a Gray code with five XOR-folds (shifts of 16, 8, 4, 2, 1); the encoding direction
  is `x ^ (x >> 1)`. Round trips are checked over the full 32-bit range at boundary values.
  *(Reference: `[@ANCHOR: gray_to_binary]`)*
- `BitWriter::write` appends a field of `width` bits MSB-first into a zeroed byte buffer, Gray-coding it when
  the width is greater than one bit. Bits are only OR-ed in, so a value that does not fit its width would
  corrupt neighbouring fields; that is a debug-asserted precondition.
  *(Reference: `[@ANCHOR: BitWriter::write]`)*
- `BitReader::read` is the reverse: it gathers `width` bits MSB-first across byte boundaries and un-Grays
  fields wider than one bit.
  *(Reference: `[@ANCHOR: BitReader::read]`)*

## Codec2 3200 fixed-point encoder

- `encoder_fixed:shift_in` slides the `i16` pitch history left by one 10 ms step and appends the new samples;
  a wrong length panics.
  *(Reference: `src/codec2_3200/encoder_fixed.rs` -> `[@ANCHOR: encoder_fixed:shift_in]`)*

## Spectral envelope (decoder)

`src/codec2_3200/envelope.rs` turns eleven LPC coefficients plus a frame energy into one amplitude per
harmonic, by sampling the LPC synthesis filter's magnitude at each harmonic and applying an energy-preserving
formant-sharpening postfilter (the classic Chen and Gersho idea with `gamma = 0.5`, `beta = 0.2`).

- `lpc_spectrum` zero-pads the coefficients into a 512-point buffer, runs a forward FFT and returns the
  first 257 bins.
  *(Reference: `[@ANCHOR: lpc_spectrum]`)*
- `lpc_spectrum_fixed` does this in Q23 with the sparse-prefix forward FFT into the shared scratch buffers
  (only the first 11 inputs are non-zero).
  *(Reference: `[@ANCHOR: lpc_spectrum_fixed]`)*
- `compute_harmonic_amplitudes` computes the power spectrum of the filter and of its bandwidth-expanded copy,
  derives a gain that keeps the postfiltered energy equal to the frame energy, and sets each harmonic's
  amplitude from the postfilter-weighted power over the bins around it (1.96 times weight below 1 kHz). It
  also returns the complex spectrum so the synthesis can read the filter's phase.
  *(Reference: `[@ANCHOR: compute_harmonic_amplitudes]`)*
- `compute_harmonic_amplitudes_fixed` is the Q23 version, replacing square roots and powers with
  `log2_q23`/`exp2_q23`, computing the per-bin postfilter weight once and reusing it for the gain and for
  every harmonic band; the gain is one exact 128-bit division.
  *(Reference: `[@ANCHOR: compute_harmonic_amplitudes_fixed]`)*
- `apply_first_harmonic_correction` multiplies the first harmonic's amplitude by 0.032 when the pitch is
  below 150 Hz (`wo < pi * 150 / 4000`), because LPC overestimates the fundamental of low voices.
  *(Reference: `[@ANCHOR: apply_first_harmonic_correction]`)*
- `apply_first_harmonic_correction_fixed` does the same comparison and scaling against Q23 constants.
  *(Reference: `[@ANCHOR: apply_first_harmonic_correction_fixed]`)*
- `sample_filter_phase` returns, for each harmonic `m`, the conjugate of the LPC spectrum at the bin nearest
  `m * wo`: the synthesis filter's phase, opposite to the analysis filter's.
  *(Reference: `[@ANCHOR: sample_filter_phase]`)*
- `sample_filter_phase_fixed` is a test-only wrapper that applies the production phase-sampling step to a
  full spectrum array (production reads the transform scratch directly).
  *(Reference: `[@ANCHOR: sample_filter_phase_fixed]`)*

## Fixed-point FFT, logarithm and trigonometry

- `ComplexQ23::mul` multiplies two Q23 complex numbers and rescales to Q23; a plain `i64` path when every
  component is under 2^30, an exact `i128` path otherwise, both bit-identical.
  *(Reference: `src/codec2_3200/fixed_fft.rs` -> `[@ANCHOR: ComplexQ23::mul]`)*
- `fixed_fft:rshift_round_i128` is the round-half-up right shift for an `i128` accumulator that narrows to
  `i64`, with a debug assertion that the result fits (the FFT butterflies are bounded far below that).
  *(Reference: `[@ANCHOR: fixed_fft:rshift_round_i128]`)*
- `build_bit_reverse_table` computes, at compile time, the bit-reversal permutation for an `N`-point
  transform as `u16` entries.
  *(Reference: `[@ANCHOR: build_bit_reverse_table]`)*
- `fft_twiddles_q23` (test helper) returns the twiddle-factor table for transform size `N`.
  *(Reference: `[@ANCHOR: fft_twiddles_q23]`)*
- `fft_bit_reverse_table` (test helper) returns the bit-reversal table for transform size `N`.
  *(Reference: `[@ANCHOR: fft_bit_reverse_table]`)*
- `log2_lut_generic_fixed` computes `log2(x)` for a positive `f32` by taking the exponent from the IEEE-754
  bits and interpolating the mantissa's logarithm in a Q23 table, using the raw mantissa bits as the
  interpolation weight; no float arithmetic until the final sum.
  *(Reference: `src/codec2_3200/fixed_point.rs` -> `[@ANCHOR: log2_lut_generic_fixed]`)*
- `f32_to_q_exact_round` converts an `f32` to a Q format with exact rounding (the magnitude is rounded half up,
  i.e. halves go away from zero) by shifting its 24-bit significand (zero maps to zero; subnormals and out-of-range values are debug-asserted).
  *(Reference: `[@ANCHOR: f32_to_q_exact_round]`)*
- `exp2_lut_generic_fixed` is the mirror image: `2^y` for an `f32`, with `y` converted exactly to Q24, split
  into a floor and a fraction, interpolated in a Q23 table and rebuilt as an IEEE-754 bit pattern.
  *(Reference: `[@ANCHOR: exp2_lut_generic_fixed]`)*
- `log2_q23` is the integer-in, integer-out base-2 logarithm of a positive Q23 value: normalise by counting
  leading zeros, table-interpolate the mantissa. Zero is treated as the smallest positive value.
  *(Reference: `[@ANCHOR: log2_q23]`)*
- `exp2_q23` is `2^y` for a Q23 exponent, returning Q23: the integer part shifts, the fractional part is
  table-interpolated; very negative `y` underflows to 0 and very large `y` is debug-asserted.
  *(Reference: `[@ANCHOR: exp2_q23]`)*
- `sin_cos_q23` returns `(cos, sin)` as a Q23 complex number for a `u32` binary angle (the full range is one
  turn), indexing a 4096-cell (4097-point) table with the top 12 bits and interpolating with the rest.
  *(Reference: `src/codec2_3200/trig_fixed.rs` -> `[@ANCHOR: sin_cos_q23]`)*

## Floating-point LPC analysis (reference, and the live 1600 encoder)

`src/codec2_3200/floating_reference/lpc.rs`.

- `autocorrelate` computes `R[j] = sum wn[i] * wn[i + j]` for lags 0 to 10 over the windowed buffer.
  *(Reference: `[@ANCHOR: autocorrelate]`)*
- `levinson_durbin` solves for the ten predictor coefficients from the autocorrelation (`ak[0] = 1`); a
  reflection coefficient larger than 1 in magnitude is replaced by 0, and the recursion stops with the
  identity filter when the residual energy reaches zero (silence) instead of dividing 0 by 0.
  *(Reference: `[@ANCHOR: levinson_durbin]`)*
- `build_p_q` forms the two symmetric and antisymmetric polynomials `P` and `Q` (the LPC filter plus and
  minus its mirror) in Chebyshev form, whose interleaved roots are the LSP frequencies.
  *(Reference: `[@ANCHOR: build_p_q]`)*
- `find_next_root` finds one root of such a polynomial by sweeping `x` downward from a start point in 0.01
  steps until the sign changes, then bisecting six times; it returns `None` when the sweep reaches -1
  without a sign change.
  *(Reference: `[@ANCHOR: find_next_root]`)*
- `lpc_to_lsp` finds the ten roots in turn, alternating between `P` and `Q`, each search starting where the
  last root was found, and returns their arccosines; `None` if fewer than ten roots are found.
  *(Reference: `[@ANCHOR: lpc_to_lsp]`)*
- `lpc_energy` is the prediction-error energy `sum ak[i] * R[i]`; it has to be taken before bandwidth
  expansion or it can come out negative.
  *(Reference: `[@ANCHOR: lpc_energy]`)*

## Pitch estimation

The pitch estimator squares the speech, removes DC, low-pass filters and decimates by 5, windows, transforms
and searches the spectrum for the strongest bin in the 50 to 400 Hz range, then corrects octave errors.

- `NlpState::default` (float reference) sets up the estimator state: empty squared history, zero filter
  memory, a previous pitch of 100 Hz, and a planned 512-point FFT.
  *(Reference: `src/codec2_3200/floating_reference/nlp.rs` -> `[@ANCHOR: NlpState::default]`)*
- `decimate` (float) low-pass filters the squared signal with the 25-tap filter and keeps every fifth
  sample, replicating the buffer edge at the borders.
  *(Reference: `[@ANCHOR: decimate]`)*
- `correct_sub_multiples` (float) walks down through integer sub-multiples of the strongest bin and prefers
  one that is a local peak and above 0.3 of the global peak (half that when it is near the previous frame's
  pitch).
  *(Reference: `[@ANCHOR: correct_sub_multiples]`)*
- `dc_notch` (float) is the first-order DC blocker `y = x - x_prev + 0.95 * y_prev`.
  *(Reference: `[@ANCHOR: dc_notch]`)*
- `nlp` (float) is the whole estimator for the newest 80 samples: square, notch, add 1, decimate, window,
  FFT, power, peak search, sub-multiple correction; it returns the pitch in Hz and remembers it.
  *(Reference: `[@ANCHOR: nlp]`)*
- `lowpass_coeffs` returns the cached 25-tap Hann-windowed sinc low-pass (cut-off 0.1 cycles per sample).
  *(Reference: `src/codec2_3200/nlp.rs` -> `[@ANCHOR: lowpass_coeffs]`)*
- `decimate_fixed` is the integer decimation, with interior fast paths that fold the symmetric taps and give
  bit-identical results to the clamped general form.
  *(Reference: `[@ANCHOR: decimate_fixed]`)*
- `nlp:fft_fixed` runs the estimator's 512-point transform on its 64 non-zero decimated inputs through the
  shared size-specialised kernel; only the magnitude is used afterwards.
  *(Reference: `[@ANCHOR: nlp:fft_fixed]`)*
- `correct_sub_multiples_fixed` is the integer sub-multiple correction on the 128-bit power spectrum, with
  0.8 and 1.2 as exact ratios 8/10 and 12/10 and the threshold kept in 128 bits.
  *(Reference: `[@ANCHOR: correct_sub_multiples_fixed]`)*
- `nlp_fixed` returns the integer estimator's pitch in Hz as an `f32` (bin times 3.125 Hz) for tests and the
  other codec modes.
  *(Reference: `[@ANCHOR: nlp_fixed]`)*

## Voicing decision

- `is_voiced` (float) judges a 10 ms block voiced when its energy is more than 12 dB above a tracked noise
  floor and its zero-crossing rate is below 0.15; the noise floor (starting at -20 dB) is updated only
  during unvoiced blocks.
  *(Reference: `src/codec2_3200/floating_reference/voicing.rs` -> `[@ANCHOR: is_voiced]`)*
- `is_voiced_fixed` is the integer decision without logarithms: the dB margin becomes a linear energy ratio
  (about 15.85), the zero-crossing test is a cross-multiplication, and an empty block is judged unvoiced.
  *(Reference: `src/codec2_3200/voicing.rs` -> `[@ANCHOR: is_voiced_fixed]`)*

## Codec2 3200 quantisers

`src/codec2_3200/quantise.rs` and `floating_reference/quantise.rs`.

- `encode_wo` quantises a pitch (`Wo` in radians) to a 7-bit index over `2*pi/160` to `2*pi/20`.
  *(Reference: `[@ANCHOR: encode_wo]`)*
- `decode_wo` maps an index back to `min + step * index`.
  *(Reference: `[@ANCHOR: decode_wo]`)*
- `decode_wo_fixed` is the same in Q23, `w0_min + w0_step * index` with no rescale.
  *(Reference: `[@ANCHOR: decode_wo_fixed]`)*
- `encode_energy` turns a linear frame energy (floored at 1e-12) into dB with the log table and quantises it
  to a 5-bit index over -10 to 40 dB.
  *(Reference: `[@ANCHOR: encode_energy]`)*
- `decode_energy` is the inverse, using the exp2 table.
  *(Reference: `[@ANCHOR: decode_energy]`)*
- `decode_energy_fixed` computes the exponent `y` from the index in Q23 and returns `exp2_q23(y)`.
  *(Reference: `[@ANCHOR: decode_energy_fixed]`)*
- `quantize_linear` is the shared uniform quantiser: `levels * (v - min) / (max - min) + 0.5`, truncated and
  clamped to `0..levels`.
  *(Reference: `[@ANCHOR: quantize_linear]`)*
- `dequantize_linear` returns `min + step * index` with `step = (max - min) / levels`.
  *(Reference: `[@ANCHOR: dequantize_linear]`)*
- `encode_lsps_delta_scalar` (float) quantises ten LSPs as indexes of 5 bits: the first as an absolute
  frequency, each later one as the delta from the previous reconstructed value.
  *(Reference: `[@ANCHOR: encode_lsps_delta_scalar]`)*
- `lsp_dim_nearest_level` (float) picks the closest of the 32 levels of one dimension, lowest index winning
  an exact tie.
  *(Reference: `[@ANCHOR: lsp_dim_nearest_level]`)*
- `decode_lsps_delta_scalar` accumulates the dequantised deltas in Hz and converts each to radians.
  *(Reference: `[@ANCHOR: decode_lsps_delta_scalar]`)*
- `lsp_dim_value_hz_q16` is the frequency in Q16 Hz of one level of one dimension (uniform steps up to the
  breakpoint, a second step size after it).
  *(Reference: `[@ANCHOR: lsp_dim_value_hz_q16]`)*
- `lsp_dim_nearest_level_q16` searches the 32 levels by Q16 distance, keeping the first level on a tie.
  *(Reference: `[@ANCHOR: lsp_dim_nearest_level_q16]`)*
- `encode_lsps_delta_scalar_fixed` converts float radians to Q23 and delegates to the integer encoder
  (`encode_lsps_delta_scalar_q23`).
  *(Reference: `[@ANCHOR: encode_lsps_delta_scalar_fixed]`)*
- `decode_lsps_delta_scalar_fixed` accumulates Q16 Hz and converts to Q23 radians once per dimension.
  *(Reference: `[@ANCHOR: decode_lsps_delta_scalar_fixed]`)*

## Decoder interpolation

`src/codec2_3200/interp.rs`: the pitch, energy and LSPs arrive per 20 ms but synthesis runs per 10 ms.

- `interp_wo` gives the interpolated half-frame its pitch. If the interpolated half is flagged voiced but both
  neighbours are unvoiced it is treated as unvoiced (the minimum pitch); otherwise it takes the midpoint of
  two voiced neighbours or whichever neighbour is voiced.
  *(Reference: `[@ANCHOR: interp_wo]`)*
- `interp_energy` is the geometric mean `sqrt(prev * next)`.
  *(Reference: `[@ANCHOR: interp_energy]`)*
- `interp_wo_fixed` is `interp_wo` in Q23; the midpoint is `(prev + next) >> 1`.
  *(Reference: `[@ANCHOR: interp_wo_fixed]`)*
- `interp_energy_fixed` computes the geometric mean as `exp2((log2 a + log2 b) / 2)` with the Q23 primitives;
  both inputs must be positive.
  *(Reference: `[@ANCHOR: interp_energy_fixed]`)*

## Spectral bridge numerics

- `extrapolate_amplitudes` fits `ln(a)` against harmonic frequency over the top `min(l, 16)` decoded
  harmonics by least squares, clamps a rising slope to flat, and continues the line out to `min(2l, 160)`
  harmonics; nothing is added unless the bridge is enabled and the sub-frame is voiced.
  *(Reference: `src/codec2_3200/spectral_bridge.rs` -> `[@ANCHOR: extrapolate_amplitudes]`)*
- `freq_hz_fixed` is the frequency of harmonic `m` in whole Hz, `m * wo * 4000 / pi`, rounded; keeping `x` a
  plain integer keeps the regression sums inside `i64`.
  *(Reference: `[@ANCHOR: freq_hz_fixed]`)*
- `spectral_bridge_div_round_i128` divides an `i128` by a positive divisor rounding to nearest, ties away from
  zero; it serves the two divisions of the fit (the lpc module has its own copy under another anchor).
  *(Reference: `[@ANCHOR: spectral_bridge_div_round_i128]`)*
- `extrapolate_amplitudes_fixed` is the same fit in base-2 logarithms: slope and intercept in Q23 log2
  amplitude, a non-rising clamp, and `exp2_q23` for each new harmonic.
  *(Reference: `[@ANCHOR: extrapolate_amplitudes_fixed]`)*

## Synthesis internals

`src/codec2_3200/synthesis.rs`.

- `make_synthesis_window` builds the overlap-add window: a trapezoid ramping up over the 80 samples around
  sample 40 and down around sample 120, which for the codec's constants is a triangle peaking at sample 80.
  *(Reference: `[@ANCHOR: make_synthesis_window]`)*
- `next_rand` advances a xorshift generator and returns a uniform angle in `[0, 2*pi)`; it need not match
  the reference generator because it only randomises phase.
  *(Reference: `[@ANCHOR: next_rand]`)*
- `next_rand_fixed` is the same state update returning the raw 32-bit word as a binary angle.
  *(Reference: `[@ANCHOR: next_rand_fixed]`)*
- `phase_increment_q32` converts `wo * 80` (the pitch phase advance per sub-frame) into a 32-bit turn
  fraction by dividing by `2*pi` in Q23 with rounding and discarding whole turns.
  *(Reference: `[@ANCHOR: phase_increment_q32]`)*
- `postfilter_step` is the decision logic of the voiced postfilter: it computes the mean harmonic energy in
  dB; on an unvoiced frame below 40 dB it updates the background estimate (beta 0.1), and on a voiced frame
  it flags harmonics quieter than the background plus 6 dB.
  *(Reference: `[@ANCHOR: postfilter_step]`)*
- `postfilter_step_fixed` is the Q23 version (`l` is clamped to at least 1 so the division cannot trap).
  *(Reference: `[@ANCHOR: postfilter_step_fixed]`)*

## Analysis windows

- `make_analysis_window` builds the 320-sample Hann window over the central 279 samples, scaled by `1 / sqrt(sum(w^2) * 512)`, the
  normalisation the LPC analysis expects.
  *(Reference: `src/codec2_3200/window.rs` -> `[@ANCHOR: make_analysis_window]`)*
- `make_analysis_window_fixed` returns the same window as the committed Q30 table.
  *(Reference: `[@ANCHOR: make_analysis_window_fixed]`)*
