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
`i128` loop gives. `bf`, `bf_one`, `bf_quarter` and `i32_twiddle_scaled` are the butterfly forms (generic, add/subtract-only for the twiddles
`1` and `-j`, and the 32-bit-halves multiply); `block` and `block_inv_i64` / `block_inv_i64_pruned` apply them across one block of a stage,
the latter two using the quarter-turn symmetry of the table. `stage_prefix`, `stages_prefix_loop`, `stages_prefix_hot`, `prefix_general`,
`scatter_prefix`, `scatter_prefix_short`, `prefix_fill_len`, `fft_fixed_sparse_prefix_forward` and `pitch_fft_512_kernel` are the transform for
an input whose only nonzero samples are the first few (the pitch estimator's 64-sample window). `SparseInverse` (`put`, `run_kernel`),
`fold_flags`, `stage_sparse`, `stages_sparse_hot`, `stages_sparse_loop`, `final_stage_real` and `twiddle_real_inverse` are the sparse harmonic
inverse that skips empty blocks. `narrow_twiddles`, `quadrant_symmetric` and `twiddles_below_unity` vet the const tables at compile time.
  *(Reference: `src/codec2_3200/fixed_fft.rs` -> `[@ANCHOR: fixed_fft:bf]`, `[@ANCHOR: fixed_fft:bf_one]`, `[@ANCHOR: fixed_fft:bf_quarter]`, `[@ANCHOR: fixed_fft:block]`, `[@ANCHOR: fixed_fft:block_inv_i64]`, `[@ANCHOR: fixed_fft:block_inv_i64_pruned]`, `[@ANCHOR: fixed_fft:fft_fixed_sparse_prefix_forward]`, `[@ANCHOR: fixed_fft:final_stage_real]`, `[@ANCHOR: fixed_fft:fold_flags]`, `[@ANCHOR: fixed_fft:i32_twiddle_scaled]`, `[@ANCHOR: fixed_fft:narrow_twiddles]`, `[@ANCHOR: fixed_fft:pitch_fft_512_kernel]`, `[@ANCHOR: fixed_fft:prefix_fill_len]`, `[@ANCHOR: fixed_fft:prefix_general]`, `[@ANCHOR: fixed_fft:quadrant_symmetric]`, `[@ANCHOR: fixed_fft:scatter_prefix]`, `[@ANCHOR: fixed_fft:scatter_prefix_short]`, `[@ANCHOR: fixed_fft:stage_prefix]`, `[@ANCHOR: fixed_fft:stages_prefix_hot]`, `[@ANCHOR: fixed_fft:stages_prefix_loop]`, `[@ANCHOR: fixed_fft:stage_sparse]`, `[@ANCHOR: fixed_fft:stages_sparse_hot]`, `[@ANCHOR: fixed_fft:stages_sparse_loop]`, `[@ANCHOR: fixed_fft:twiddle_real_inverse]`, `[@ANCHOR: fixed_fft:twiddles_below_unity]`, `[@ANCHOR: fixed_fft:SparseInverse_put]`, `[@ANCHOR: fixed_fft:SparseInverse_run_kernel]`)*

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
