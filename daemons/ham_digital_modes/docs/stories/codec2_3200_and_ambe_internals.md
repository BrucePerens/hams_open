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
