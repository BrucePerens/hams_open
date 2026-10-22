## AMBE (TIA-102.BABA) float codec internals: bit handling, pitch, voicing, quantization and synthesis

These are the internal steps of the float P25 full-rate codec in `src/ambe/float/tia_102_baba/`. The user-visible path through
them (receive, decode, transmit) is told in `docs/stories/ambe_p25_voice_frame_wire_path.md`; this section covers the helpers
and the equations behind each stage, in the order the encoder meets them. Equation numbers refer to TIA-102.BABA_2003.

### Bit prioritization helpers

`raster_unscan_bits` is the inverse of the raster scan: given the flat scanned bits and the column widths, it walks the same
level-by-level, column-by-column order and rebuilds each column's value. It returns `None` if the bit count does not match the
widths exactly (too few bits, or bits left over).
*(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: raster_unscan_bits]`)*

`read_bits` reads the next `n` bits of a bit slice, most significant first, as an integer and moves the cursor; it returns `None`
if fewer than `n` bits remain. `deprioritize_bits` uses it for each fixed-width field.
*(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: read_bits]`)*

`set_bit` writes one bit of the de-interleaved code vectors: it sets bit `source.1` (0 = least significant) of vector
`source.0` when the received symbol bit is 1, and does nothing for a 0 (the vectors start as zero).
*(Reference: `src/ambe/float/tia_102_baba/interleave.rs` -> `[@ANCHOR: set_bit]`)*

### Pitch analysis (section 5.1)

The analysis works on a lowpass-filtered copy of the speech. `lowpass_filtered_sample` computes one sample of it (Eq. 9): the sum
of 21 input samples around `center + n`, weighted by the symmetric Annex D filter taps. It indexes the raw buffer directly, so the
buffer must extend at least 10 samples past both ends of the range being asked for, or it panics.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: lowpass_filtered_sample]`)*

`PitchAnalysisFrame::new` prepares everything the pitch error function needs once per frame: the 301 lowpassed samples around
`center`, their windowed energy with the Annex B window, the sum of the window's fourth powers, and the autocorrelation table
`r(t)` of the doubly-windowed signal for integer lags 0 to 151 (Eq. 7; terms that fall outside the window vanish). The raw buffer
needs 160 samples of margin on each side of `center` (150 for the window plus 10 for the filter).
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: PitchAnalysisFrame::new]`)*

`PitchAnalysisFrame::r` returns `r(t)` for a fractional lag by linear interpolation between the two neighbouring table entries
(Eq. 8); the table only holds non-negative lags because `r` is even.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: PitchAnalysisFrame::r]`)*

`PitchAnalysisFrame::error_function` is `E(P)` of Eq. 5 for a candidate period `P` in samples: the energy minus `P` times the sum
of `r(nP)` over the multiples of `P` that fit in the window, divided by the energy times `1 - P * sum(w^4)`. Small is good. Two
deliberate departures are in the code and the comment above it: a value that comes out slightly negative is raised to 0 (the look-ahead
ratio tests would otherwise be ill-defined), and a silent frame (denominator near zero) returns 1.0, the worst error, instead of
`0/0`.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: PitchAnalysisFrame::error_function]`)*

`look_back_pitch_tracking` (Eq. 10-12) looks for continuity with the past. Among the candidate periods 21, 21.5, ..., 122 that lie
within 0.8 to 1.2 times the previous frame's pitch, it takes the one with the smallest current error `E(P)` and returns it with the
cumulative error `CE_B`, which is that error plus the previous two frames' errors. A stream's first frames pass `(100.0, 0.0)` for
both previous entries, the standard's initial values. It panics if no candidate lies within 0.8 to 1.2 times the previous pitch, which cannot happen when that pitch is itself a candidate.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: look_back_pitch_tracking]`)*

`look_ahead_pitch_tracking` (Eq. 13-20) looks at the two following frames, whose error functions can already be computed. For each
candidate `P0` it finds the cheapest chain of pitches for the next two frames, each within 0.8 to 1.2 times the one before, and the
cumulative error `CE_F` is the sum of the three errors. It picks the `P0` with the smallest `CE_F`, then tests its sub-multiples
`P0/2, P0/3, ...` (snapped to the candidate grid, smallest first) against the three accept rules of Eq. 18 to 20 so that a pitch
doubled by mistake is corrected. It returns the chosen period and its `CE_F`.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: look_ahead_pitch_tracking]`)*

`choose_initial_pitch_estimate` (Eq. 21-23) makes the final pick between the two trackers: the look-back pitch if its cumulative
error is at most 0.48 or no larger than the look-ahead error, otherwise the look-ahead pitch. The first two conditions are kept as
the standard's two separate rules even though they choose the same thing.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: choose_initial_pitch_estimate]`)*

### Pitch refinement (section 5.1.5)

`window_dft_16384` is `W_R(m)` (Eq. 30), the spectrum of the refinement window at a 16384-point resolution. It sums the cosine
terms of the window's 221 non-zero samples directly (the sine terms cancel by symmetry) instead of running a transform; because
it is even in `m` and queried thousands of times per frame, the half-line `m = 0..=8192` is tabulated once per process and
anything beyond is computed on demand.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: window_dft_16384]`)*

`RefinementFrame::new` computes the frame's spectrum `S_w(m)` (Eq. 29): the 256-point DFT of the 221 speech samples around
`center` multiplied by the refinement window, for bins `m = -127..=128`, once per frame. It needs 110 samples of margin on both
sides of `center` and panics otherwise.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: RefinementFrame::new]`)*

`RefinementFrame::sw_at` returns one bin of that spectrum and gives zero outside `-127..=128`, so loops near the edge of the pitch
range read "no energy there" instead of panicking.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: RefinementFrame::sw_at]`)*

`harmonic_amplitude` estimates the complex amplitude `A_l(omega0)` of harmonic `l` (Eq. 26-28) as a least-squares fit: the sum
over the bins of the harmonic's band of `S_w(m)` times the window spectrum `W_R` at the matching offset, divided by the sum of
`W_R` squared. An empty or vanishingly narrow band returns zero.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: harmonic_amplitude]`)*

`refinement_error` is `E_R(omega0)` (Eq. 24): the squared distance between the real spectrum and the synthetic harmonic spectrum
for a candidate pitch, summed from bin 50 up to the last bin covered by the harmonics it implies. The encoder tries candidates
around the half-sample estimate and keeps the one with the smallest error.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: refinement_error]`)*

### Voiced/unvoiced decision (section 5.2)

`frequency_bands_count` is `K_hat` (Eq. 34): the number of V/UV bands, `ceil(L/3)` for up to 36 harmonics and 12 above that.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: frequency_bands_count]`)*

`voicing_measure` is `D_k` (Eq. 35, and Eq. 36 for the top band): the share of a band's spectral energy that the harmonic model
fails to explain, `sum |S_w - synthetic|^2 / sum |S_w|^2` over the band's bins. Near 0 means voiced, near 1 unvoiced. A band with no
energy at all counts as 1 (unvoiced). The top band extends to the last harmonic.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: voicing_measure]`)*

`update_xi_max` tracks the loudness reference `xi_max` (Eq. 41): when the current frame energy `xi_0` exceeds it, the new value is
the average of the two; otherwise it decays as `0.99 * old + 0.01 * xi_0` and is never allowed below 20000.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: update_xi_max]`)*

`energy_dependent_function` is `M(xi)` (Eq. 42): `(0.0025 xi_max + xi_0) / (0.01 xi_max + xi_0)`, scaled down by
`sqrt(xi_LF / (5 xi_HF))` when the low-frequency energy is under five times the high-frequency energy.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: energy_dependent_function]`)*

`voicing_threshold` is `Theta(k, omega0)` (Eq. 37): band `k` is voiced when its measure is below it. It is zero for every band
after the first if the initial pitch error exceeds 0.5; otherwise `0.5625` (band was voiced in the previous frame) or `0.45` times
`1 - 0.3096 (k - 1) omega0` times `M(xi)`.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: voicing_threshold]`)*

`determine_voicing` runs the whole decision for a frame (Fig. 11): it computes `L` and `K`, the energy terms and `M(xi)`, and then
one voiced/unvoiced flag per band. It returns the flags and the updated `xi_max`. A previous frame with fewer bands (or none) reads
as unvoiced for the missing bands, which is this implementation's choice since the standard is silent on it.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: determine_voicing]`)*

`encode_voicing_decisions` packs the per-band flags into `b_hat_1` (Eq. 49): band 1 is the most significant of the `K` bits.
*(Reference: `src/ambe/float/tia_102_baba/parameter_encoding.rs` -> `[@ANCHOR: encode_voicing_decisions]`)*

### Spectral amplitude estimation (section 5.3)

`voiced_amplitude` (Eq. 43) is the square root of the ratio of the signal's energy in the harmonic's band to the window spectrum's
energy over the same bins, so a pure sinusoid yields its amplitude. A band without window energy gives zero.
*(Reference: `src/ambe/float/tia_102_baba/spectral_amplitude.rs` -> `[@ANCHOR: voiced_amplitude]`)*

`unvoiced_amplitude` (Eq. 44) is the root of the band's mean energy per bin divided by the window's tap sum, a flat noise-like
level; an empty band gives zero.
*(Reference: `src/ambe/float/tia_102_baba/spectral_amplitude.rs` -> `[@ANCHOR: unvoiced_amplitude]`)*

`estimate_spectral_amplitudes` estimates `M_hat_l` for every harmonic `1..=L`, picking the voiced or unvoiced estimator by the
harmonic's band, `min(ceil(l/3), K)`.
*(Reference: `src/ambe/float/tia_102_baba/spectral_amplitude.rs` -> `[@ANCHOR: estimate_spectral_amplitudes]`)*

### Prediction, gain/block transforms and quantization (section 6)

The encoder does not send amplitudes directly; it sends the change from a prediction made out of the previous frame.
`prediction_coefficient` is `rho` (Eq. 55): 0.4 for up to 15 harmonics, `0.03 L - 0.05` for 16 to 24, and 0.7 above.
*(Reference: `src/ambe/float/tia_102_baba/prediction.rs` -> `[@ANCHOR: prediction_coefficient]`)*

`previous_log2_amplitude` returns `log2` of the previous frame's amplitude at index `j`, applying the standard's boundary rules:
index 0 gives exactly 0 (Eq. 56) and indexes past the previous frame's last harmonic repeat that last value (Eq. 57).
*(Reference: `src/ambe/float/tia_102_baba/prediction.rs` -> `[@ANCHOR: previous_log2_amplitude]`)*

`predicted_and_bias_correction` returns the two terms that the encoder (Eq. 54) and decoder (Eq. 77) both need, once: the previous
log amplitude interpolated at harmonic `l`'s position in the previous frame's harmonic scale (Eq. 52-53) times `rho`, and the
frame-wide bias, `rho / L` times the same interpolation summed over all harmonics. The two directions use them with opposite signs.
*(Reference: `src/ambe/float/tia_102_baba/prediction.rs` -> `[@ANCHOR: predicted_and_bias_correction]`)*

`gain_vector_dct` is the second-stage 6-point transform of Eq. 61: `G_m = (1/6) sum R_i cos(pi (m-1)(i-0.5) / 6)` over the six
per-block DC terms.
*(Reference: `src/ambe/float/tia_102_baba/mod.rs` -> `[@ANCHOR: gain_vector_dct]`)*

`partition_into_blocks` splits the `L` prediction residuals into six consecutive blocks whose lengths come from Annex J
(Eq. 58-59). It returns `None` for `L` outside 9 to 56 or a residual slice of the wrong length.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: partition_into_blocks]`)*

`block_dct` is the per-block DCT of Eq. 60: `C_k = (1/J) sum c_j cos(pi (k-1)(j-0.5) / J)` for a block of any length `J`; an empty
block gives an empty result.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: block_dct]`)*

`saturating_uniform_quantize` is the quantizer shared by Eq. 62 and 63: it divides by the step size, floors, and offsets by
`2^(bits-1)`, with values below the range going to 0 and values above to `2^bits - 1`. It needs at least 1 bit: Annex F never allocates 0 bits to a gain element, and the higher-order caller skips 0-bit entries before calling it.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: saturating_uniform_quantize]`)*

`quantize_gain_vector` quantizes the five non-DC gain elements `G_2..G_6` with the Annex F bit count and step size for the frame's
`L`, returning each value together with its width so the bit-prioritization step cannot be given mismatched widths. `None` for
`L` outside 9 to 56.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: quantize_gain_vector]`)*

`higher_order_coefficient_positions` lists the `(block, position)` of every higher-order DCT coefficient `C_{i,k}` for
`k >= 2`, in the order Annex G allocates bits: all of block 1 first, then block 2, and so on.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: higher_order_coefficient_positions]`)*

`quantize_higher_order_coefficients` quantizes those coefficients (Eq. 63). The step size is the Table 3 multiplier for the
coefficient's bit count times the Table 4 standard deviation for its position. Coefficients allocated 0 bits are not transmitted
and are left out of the result. It returns `None` for an out-of-range `L` or inconsistent tables.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: quantize_higher_order_coefficients]`)*

### Reconstruction (decoder, and the encoder's local decoder)

`dequantize_uniform` is the reverse of the quantizer, for Eq. 68 and 71: the centre of the quantizer bin, `step * (q - 2^(bits-1) +
0.5)`, and 0 when no bits were sent.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: dequantize_uniform]`)*

`reconstruct_gain_vector` rebuilds `G_1..G_6` (Eq. 68): `G_1` is the Annex E level at index `b_hat_2`, and `G_2..G_6` are
dequantized with the same widths and steps the quantizer used. `None` for an invalid `b2` or `L`.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: reconstruct_gain_vector]`)*

`inverse_gain_vector_dct` recovers the six block DC terms `R_i` from `G` (Eq. 69-70), the inverse of `gain_vector_dct`.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: inverse_gain_vector_dct]`)*

`inverse_block_dct` is the inverse of `block_dct` for one block of any length (Eq. 73-74).
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: inverse_block_dct]`)*

`reconstruct_higher_order_coefficients` dequantizes all higher-order coefficients (Eq. 71-72) into six blocks, taking one value per
position that was allocated bits and leaving zero-bit positions at 0. The first slot of each block, the DC term, stays 0 here and is
filled in by the caller from `R_i`. It returns `None` if the number of values does not match the positions that carry bits.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: reconstruct_higher_order_coefficients]`)*

`reconstruct_spectral_amplitudes` is the full chain (Eq. 67-79): gain vector, inverse gain DCT, higher-order coefficients with
`R_i` inserted as each block's DC term, inverse block DCTs to the residuals `T_l`, then adding the prediction from the previous
frame in the log2 domain and exponentiating to get `M_l` for `l = 1..=L`. The encoder runs it too so its history matches the
decoder's. `None` for invalid inputs.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: reconstruct_spectral_amplitudes]`)*

### Annex tables and lookups

`quantize_gain_index` returns `b_hat_2`: the index of the Annex E level (64 of them) closest to `G_1`. On an exact tie the lower
index wins.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: quantize_gain_index]`)*

`block_lengths_for_l` returns the six block lengths of Annex J for `L` in 9 to 56 (they sum to `L` and never decrease with
frequency), and `None` outside that range.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: block_lengths_for_l]`)*

`gain_bit_allocation` returns Annex F's bit count and step size for gain element `m` (2 to 6) at harmonic count `L`; `None`
outside those ranges.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: gain_bit_allocation]`)*

`higher_order_bit_allocation` returns Annex G's list of `L - 6` bit counts, one per higher-order coefficient in the order of
`higher_order_coefficient_positions`. Some entries are 0 (not transmitted). `None` outside 9 to 56.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: higher_order_bit_allocation]`)*

`higher_order_step_multiplier` is Table 3: the step size, as a multiple of the coefficient's standard deviation, for 1 to 10 bits
(for example 0.40 at 4 bits). `None` for 0 bits or more than 10.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: higher_order_step_multiplier]`)*

`higher_order_coefficient_sigma` is Table 4: the standard deviation of the `k`th coefficient of a block, for `k` 2 to 10, the same
for every block. `None` otherwise.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: higher_order_coefficient_sigma]`)*

### Spectral amplitude enhancement and smoothing (sections 8 and 9)

The decoder improves the reconstructed amplitudes before synthesis, without feeding the result back into its prediction history.
`weight` is `W_l` of Eq. 107, the raw enhancement factor for a harmonic, computed from its amplitude and the two frame energies
`R_M0` and `R_M1`. It must not be called with `R_M0 = 0`.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: weight]`)*

`enhance_spectral_amplitudes` (Eq. 105-110) leaves harmonics up to `L/8` alone, multiplies the others by `W_l` limited to 0.5 to
1.2, and then rescales everything so the total energy equals the input's. A silent frame is returned unchanged, since the formulas
would otherwise divide zero by zero.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: enhance_spectral_amplitudes]`)*

`adaptive_voicing_threshold` is `V_M` (Eq. 112): infinite (never force a decision) for a clean frame (rate at most 0.005 and at most
4 errors), `45.255 S_E^0.375 / exp(277.26 rate)` for a mildly damaged one (rate at most 0.0125 and no errors corrected in `u_hat_4`), and `1.414 S_E^0.375` otherwise.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: adaptive_voicing_threshold]`)*

`update_amplitude_threshold` is `tau_M` (Eq. 115): 20480 for a clean frame (rate at most 0.005 and at most 6 errors), otherwise
`6000 - 300 * errors + previous tau_M`.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: update_amplitude_threshold]`)*

`amplitude_smoothing_scale` is `gamma_M` (Eq. 116): 1 when `tau_M` exceeds the amplitude sum, otherwise `tau_M / A_M`.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: amplitude_smoothing_scale]`)*

`SynthesisState::finalize_parameters` chains those steps into the final synthesis inputs: enhancement, then the `S_E` update and
the voicing smoothing with `V_M`, then the amplitude sum, `tau_M` and `gamma_M` scaling. It stores the result for a later frame
repeat and returns the smoothed voicing and final amplitudes, or `None` if the two input lengths differ.
*(Reference: `src/ambe/float/tia_102_baba/synthesis.rs` -> `[@ANCHOR: SynthesisState::finalize_parameters]`)*

### Speech synthesis (section 11)

`SynthesisState::synthesize_core` produces 160 samples from final parameters: it advances the shared noise generator by one frame
(except before the first), runs unvoiced and voiced synthesis and adds them as Eq. 142, the unvoiced part times a per-mode gain
that is 1 for the standard. Normal frames and repeated frames both end here.
*(Reference: `src/ambe/float/tia_102_baba/synthesis.rs` -> `[@ANCHOR: SynthesisState::synthesize_core]`)*

`synthesis_window` is the Annex I window `w_S(n)`: a trapezoid equal to 1 for `|n| <= 55`, falling linearly to 0 at `|n| = 105`,
and 0 outside. It is computed by formula instead of table.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: synthesis_window]`)*

The unvoiced half uses a deterministic noise sequence (Eq. 117, `u(n+1) = (171 u(n) + 11213) mod 53125`, starting at 3147).
`NoiseState::new` builds the generator's window of 209 values, `u(-104)..u(104)`, by stepping the recurrence 209 times.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::new]`)*

`NoiseState::step` advances the recurrence one value and slides the window by one.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::step]`)*

`NoiseState::advance_frame` moves the window on by one frame, 160 steps.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::advance_frame]`)*

`NoiseState::at` reads `u(relative)` for `relative` in `-104..=104` and returns `None` outside it. Both the unvoiced transform and the
voiced phase randomisation read the same window.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::at]`)*

`unvoiced_dft` is `U_w(m)` (Eq. 118): the 256-point DFT of the windowed noise for `m = -128..=127` (note the one-bin offset from the
analysis spectrum's range). It uses a shared table of the 256 complex exponentials.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_dft]`)*

`unvoiced_scaling_coefficient` is the constant `gamma_w` (Eq. 121) relating the analysis and synthesis windows, about 146.64,
computed once from the two windows.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_scaling_coefficient]`)*

`unvoiced_spectrum` builds the shaped noise spectrum (Eq. 119-120, 124): all bins start at zero; for each *unvoiced* harmonic, the
noise spectrum inside its band is rescaled so its mean power matches that harmonic's amplitude, and the matching negative-frequency
bins get the same scaling. Bands that reach past the Nyquist bin are clipped. It returns `None` if the two input slices differ in
length.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_spectrum]`)*

`unvoiced_time_domain` inverts that spectrum with a 256-point inverse DFT (Eq. 125) and keeps the real part, which is the whole
result because the spectrum is conjugate-symmetric.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_time_domain]`)*

`time_domain_at` reads one sample of that 256-sample result for `n` in `-128..=127`, and returns 0 outside.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: time_domain_at]`)*

`UnvoicedState::synthesize` produces the 160 unvoiced samples (Eq. 117-126): it builds this frame's time-domain noise and
overlap-adds it with the previous frame's, each weighted by the synthesis window and normalised by the sum of the squared windows.
The current result becomes the next call's history. `None` if the voicing and amplitude slices differ in length.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: UnvoicedState::synthesize]`)*

`delta_omega` is `Delta omega_l` of Eq. 137-138, the small frequency correction that makes a harmonic's phase ramp across the frame
land exactly on the target phase: the phase error, after removing the nominal advance and wrapping into `[-pi, pi)`, divided by 160.
It is a per-harmonic constant, so it is computed once per harmonic.
*(Reference: `src/ambe/float/tia_102_baba/voiced_synthesis.rs` -> `[@ANCHOR: delta_omega]`)*

`VoicedState::synthesize` produces the 160 voiced samples (Eq. 127-141) as a sum of sinusoids, one per harmonic. First every
harmonic's phase is advanced (the phase accumulator `psi`, plus a random offset from the shared noise for harmonics above `L/4`
that are in use). Then each harmonic is rendered according to whether it was voiced in the previous frame and is voiced now: not at
all, faded out under the window, faded in under the window, or, if voiced in both, either the two halves cross-faded (for harmonics
8 and up, or a pitch change of 10% or more) or one continuous sinusoid with interpolated amplitude and frequency. Everything is
multiplied by 2. It then stores the frame's parameters as the history. It returns `None` for mismatched slices or more than 56
harmonics.
*(Reference: `src/ambe/float/tia_102_baba/voiced_synthesis.rs` -> `[@ANCHOR: VoicedState::synthesize]`)*
