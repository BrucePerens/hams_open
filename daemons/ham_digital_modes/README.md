# ham_digital_modes

Amateur-radio digital-mode encoders and decoders as a library crate (LGPL-3.0-or-later): PSK31, RTTY, FT8, WSPR, the D-STAR header,
Codec2 1600/3200 voice frames, and the AMBE voice codec family (D-STAR, P25 and, behind the `ambe_plus_2` feature, AMBE+2 half-rate).
The hams_local_relay daemon depends on it as a pinned git dependency. `--no-default-features` builds only the fixed-point Codec2 3200
path as `no_std` (see `docs/CODEC2_NO_STD.md`); everything else needs the default `std` feature. `cargo test` runs the unit and
integration tests; `cargo test --features ambe_plus_2` also builds the gated half-rate code.

This README holds the plumbing documentation: the internal DSP steps, tables and arithmetic helpers that no operator ever calls
directly, each cited by its semantic anchor (ADR 0090). What an operator gets from each mode (encode a message to audio or bits, decode
audio to text, the frame layouts) is in the user-visible stories under `docs/stories/`:

- `psk31_text_to_audio_and_back.md`, `rtty_text_to_audio_and_back.md`, `ft8_message_codec.md`, `wspr_transmit_and_receive.md`,
  `dstar_header_framing.md`, `signal_timing_characterizer.md`
- `codec2_voice_frames.md`, `codec2_3200_and_ambe_internals.md`
- `ambe_p25_voice_frame_wire_path.md`, `ambe_dstar_and_half_rate_frame_layout.md`, `ambe_pitch_refinement.md`

# Internals

## PSK31 receive internals

`char_to_code` looks up the Varicode codeword and bit count for an ASCII byte below 128 and gives nothing for
anything higher, which is how the encoder skips non-ASCII input.
[@ANCHOR: char_to_code]

`correlate_symbol_iq` correlates one symbol's worth of samples against a cosine and a sine at the carrier and
returns the two sums (I, Q). Each sample is weighted by the same raised-cosine envelope the modulator applied,
and the carrier phase is computed from the absolute sample index of the stream (`sample_idx_start` plus the
position in the chunk), so consecutive calls stay phase-continuous. Both the whole-buffer demodulator and the
streaming decoder call it, so the constellation points shown to the operator are the numbers the bits were
decided from.
[@ANCHOR: correlate_symbol_iq]

`phase_to_bit` makes the BPSK31 bit decision: the phase change from the previous symbol, wrapped to
[-pi, pi], decides 1 if its magnitude is under pi/2 and 0 otherwise. With no previous symbol it returns 1,
which is correct because every codeword starts with a 1 and the modulator starts at zero phase.
[@ANCHOR: phase_to_bit]

`VaricodeAccumulator::push_bit` is the streaming form of the boundary logic in `psk31_decode_bits`. It takes one
bit, and on the second consecutive zero it drops the first gap zero from the accumulator, looks the codeword
up, resets, and returns the character if it matched. Otherwise it returns nothing. As in the batch decoder, a
run beyond 11 accumulated bits resets the accumulator.
[@ANCHOR: VaricodeAccumulator::push_bit]

## RTTY framing and synthesis internals

`push_framed_char` appends one character's seven bit periods to a bit vector: a SPACE start bit, the five
data bits least significant first (true is MARK), and a MARK stop bit. The stop bit is one entry here; the
audio stage stretches it to 1.5 bit periods.
[@ANCHOR: push_framed_char]

`framed_bits_to_audio` is the FSK synthesis behind `rtty_modulate`, kept separate so tests can feed it bit
sequences that the text path would never produce. MARK is `mark_hz` and SPACE is `mark_hz + 170`. The phase
accumulator runs across all bits so the waveform is continuous, and each bit lasts `sample_rate / 45.45`
samples, rounded per bit. Every seventh bit (index modulo 7 equal to 6) is taken to be a stop bit and is
lengthened to 1.5 times, so it assumes the input is built from 7-bit frames.
[@ANCHOR: framed_bits_to_audio]

## RTTY correlators

`window_mark_space_energy` is the slow reference correlator. For one window it computes the quadrature
(I and Q) correlation against the MARK and SPACE tones, recomputing sine and cosine for every sample, and
returns whether MARK is stronger together with both energies (I squared plus Q squared). It is not used on the
decode path; the tests compare it with the table-driven version.
[@ANCHOR: window_mark_space_energy]

`TrigTables::new` precomputes cosine and sine values for the MARK and SPACE tones at window-relative sample
offsets 0 to `capacity`. The phase always counts from the window's first sample, so the tables serve a window
starting anywhere in the stream.
[@ANCHOR: TrigTables::new]

`TrigTables::window_evidence` correlates `samples[start..start+len]` against the tables and returns the
signed MARK-to-SPACE ratio in dB (positive means MARK is stronger; energies are floored at 1e-15) together
with the linear total MARK plus SPACE energy. It returns `None` if `len` is zero, the start is past the end,
or more than 10 percent of the window is missing. A window short by up to 10 percent, and one longer than the
tables, is shortened to what is available; this lets the last character of a whole-buffer decode through.
[@ANCHOR: TrigTables::window_evidence]

`TrigTables::signed_ratio_db` is `window_evidence` with the energy dropped, for the many callers that need only
the ratio.
[@ANCHOR: TrigTables::signed_ratio_db]

`TrigBank::new_with_offsets` builds one `TrigTables` per carrier offset in the given list, each with MARK at
the configured frequency plus that offset and SPACE 170 Hz above it. It keeps the offset list so a decoder
can report the real offset of the table that won.
[@ANCHOR: TrigBank::new_with_offsets]

`TrigBank::new` is that bank over the standard nine offsets, -40 to +40 Hz in 10 Hz steps.
[@ANCHOR: TrigBank::new]

## RTTY edge detection and scan

`pre_is_persistently_mark` checks that the bit period just before a candidate start edge is steady MARK: it
splits that period into two sub-windows and requires each to read more than 4 dB toward MARK. This rejects
candidates whose context is not an idle or previous stop bit. An edge within one bit period of the start of the
buffer passes without a test.
[@ANCHOR: pre_is_persistently_mark]

`frame_score` rates a candidate start edge using the whole 7-bit frame, after the persistence check. It
correlates the start bit window, five data bit windows and the 1.5-bit stop window, each from the table passed
in. It returns three numbers: an accept score, the sum of the start (SPACE-ward), stop (MARK-ward) and
five data-bit ratios, each clipped to 18 dB, which decides whether a character is there; a raw unclipped
start-plus-stop score, used to find the exact edge position; and the total linear energy, used to choose
between frequency candidates. It returns `None` if the persistence check fails or any window runs past the
buffered audio.
[@ANCHOR: frame_score]

`best_frame_evidence` evaluates `frame_score` at one position for every table in the bank and keeps only those
whose accept score is above 80 dB. Of those it returns the one with the greatest linear energy, with its index.
The energy chooses the winner because a clipped score saturates and cannot tell neighbouring offsets apart.
[@ANCHOR: best_frame_evidence]

`ScanState::new` creates the scan state for a decoder: position zero, letters shift, no non-digit figures run,
and the given initial locked-offset index (the default state uses the middle, 0 Hz, entry).
[@ANCHOR: ScanState::new]

`rtty_scan` is the loop shared by the whole-buffer and streaming decoders. It steps through the buffer in
steps of one eighth of a bit period. At each position `best_frame_evidence` is tried against the full
offset bank. When it finds a candidate, the table is fixed for that character, and the position is refined
over plus or minus half a bit by taking the highest raw score. `attempt_character` then makes the hard decision,
and on success the decoder emits the text, records the winning offset and jumps past the character. A failed
attempt just moves on one step. In streaming mode it stops, leaving the position unchanged, when the
character plus the refinement margin is not fully buffered yet, so the next call retries the same position. A
zero-length bit window returns nothing.
[@ANCHOR: rtty_scan]

## RTTY presence signature

`characterize_rtty_signature` is a cheap, decode-free probe that is not wired into any decoder. It cuts the
audio into whole one-bit-period hops and takes the sign of the MARK/SPACE ratio of each, and reports the
fraction of adjacent hops that differ and a histogram of run lengths (runs of 8 hops or more share the last
bucket). It returns `None` with fewer than two hops or when the sample rate is too low for one sample per bit.
Measurement showed that keyed RTTY and noise both flip about half the time, so the one case it separates cleanly
is a steady unkeyed carrier, which never flips.
[@ANCHOR: characterize_rtty_signature]

## FT8 waveform internals

`gfsk_pulse` is the Gaussian-smoothed frequency pulse for one FT8 symbol, with a bandwidth-time product of
2.0: half the difference of two error functions, `(erf(k*BT*(t+0.5)) - erf(k*BT*(t-0.5))) / (2T)`, where t is
in symbol periods, T is the symbol period in seconds and k is pi times the square root of 2 over ln 2. It
integrates to 1 over time, so a symbol's frequency offset adds up to the tone number.
[@ANCHOR: gfsk_pulse]

## WSPR encode internals (`wspr.rs`)

The two character tables behind the packers use different digit and letter codes. [@ANCHOR: callsign_char_code] maps
a digit to 0-9, a space to 36 and a capital letter to 10-35, and anything else (including lower case) to nothing;
callers upper-case first. [@ANCHOR: grid_char_code] maps a digit to 0-9, a space to 36 and a letter to 0-25 (so `A`
is 0, not 10), which is what the Maidenhead arithmetic in the grid packer needs.

[@ANCHOR: interleave_permutation] produces the interleaver's order: it runs the integers 0..255 through an 8-bit bit
reversal and keeps, in order, the results below 162. Coded bit `p` is sent in symbol position `perm[p]`, so the
result is a bijection on 0..161 (tested), and the decoder undoes it by reading position `perm[p]` for bit `p`.

[@ANCHOR: convolutional_encode] is the rate-1/2, constraint-length-32 encoder. For each of the 88 input bits, taken
most significant bit first from the 11 bytes, it shifts the bit into a 32-bit register and emits the parity of the
register ANDed with `POLY1` (`0xF2D05351`) and then with `POLY2` (`0xE4613C47`). The input holds 50 payload bits and
38 zero bits (6 to the byte boundary and 32 to flush the register), giving 176 coded bits of which only the first 162
are transmitted.

[@ANCHOR: wrap_mono_i16_as_wav] writes the WAV container used by the test fixtures: `RIFF` with length
`36 + data`, `WAVE`, a 16-byte PCM `fmt ` chunk (mono, the given sample rate, byte rate twice the sample rate, block
align 2, 16 bits) and a `data` chunk with the samples in little-endian order, so `data` always starts at byte 36 and
the samples at byte 44.

The noise generator for the fixtures is a SplitMix64 pseudo-random generator, deterministic for a given seed and not
for cryptographic use. [@ANCHOR: SplitMix64::next_u64] adds the golden-ratio constant `0x9E3779B97F4A7C15` to the
state and scrambles it with two xor-shift-multiply rounds, returning 64 bits. The Gaussian samples come from
[@ANCHOR: SplitMix64::next_gaussian], which draws two uniform numbers in (0, 1] (never 0, so the logarithm is
finite) and applies the Box-Muller transform, using only the cosine branch: `sqrt(-2 ln u1) * cos(2 pi u2)`.

## WSPR sequential decode internals (`wspr_decode.rs`)

[@ANCHOR: fano_bit_metric] is the branch metric of sequential decoding for one coded bit: `log2(P(r|hyp) / P(r)) - R`
with `R = 0.5` bits per coded bit for a rate-1/2 code, where the received value `r` is modelled as the bipolar
amplitude (+ for 1, - for 0) plus Gaussian noise of the given standard deviation, and `P(r)` averages the two
hypotheses. It subtracts the larger of the two Gaussian exponents before exponentiating so that a received value far
from both hypotheses gives a finite metric instead of `0/0`; the normalising constant cancels and is left out. A
correct path therefore gains metric and a wrong one loses it.

[@ANCHOR: deinterleave_symbol_values] converts values indexed by transmitted symbol position into values indexed by
coded-bit position, `out[p] = in[perm[p]]`, using the same permutation function as the encoder so it is the exact
inverse. The decoder is given the data-bit soft value of each symbol, never the sync bit.

## WSPR sync search internals (`wspr_sync.rs`)

[@ANCHOR: samples_per_symbol] is the rounded `sample_rate / (12000/8192)`, 8192 at 12 kHz. It is also the FFT length,
because WSPR's tone spacing equals its symbol rate, so every FFT bin is exactly one tone wide.

[@ANCHOR: spectrum_matrix] builds the 162-row table of magnitudes for a start position: row `s` is the spectrum of
the block starting at `start + s * samples_per_symbol`, restricted to bins `bin_lo..=bin_hi_inclusive` and
shifted down by a sub-bin frequency offset first. It returns nothing, rather than panicking, when the window runs
past the end of the audio or the bin range is empty or beyond the FFT length. The per-row work is
[@ANCHOR: block_spectrum]: multiply the block by `exp(-j 2 pi offset n / sample_rate)` (phase restarting at zero
for each block, so the result depends only on the block position and offset, which lets `find_sync` cache blocks),
take the FFT with a plan supplied by the caller, and return the magnitudes of the requested bins.

[@ANCHOR: tone_magnitudes_from_matrix] cuts four consecutive columns starting at `base_bin` out of each row of that
table, giving the magnitudes of tones 0-3 for every symbol; `base_bin` must not be below `bin_lo`.

The score for a candidate is built from three pieces. [@ANCHOR: sync_pattern] is the sync vector as +1/-1.
[@ANCHOR: sync_statistic] gives per symbol `max(tone 1, tone 3) - max(tone 0, tone 2)`, which is positive when the
sync bit is 1 and negative when it is 0 if the window is aligned, whatever the data bit. [@ANCHOR: cosine_score]
compares the two as `sum(tt * pattern) / (|tt| * |pattern|)`, in -1..1, and gives 0 for an all-zero statistic. It
replaced a mean-absolute-value normalisation, which scored the maximum whenever all signs merely agreed and so
accepted noise.

[@ANCHOR: extract_all_four_tone_magnitudes] is the shared extraction step: after rejecting a zero sample rate, a
non-finite or negative `base_hz`, or one above half the sample rate, it takes the nearest bin to `base_hz`, applies
the remaining fraction as the sub-bin offset, plans an FFT and returns all four tone magnitudes of the 162 symbols
(or nothing for a short buffer). [@ANCHOR: extract_impossible_tone_evidence] returns the two tones that cannot be
present given the known sync bit (indices `1 - sync` and `3 - sync`), a noise-only reference per symbol.

Two ways to turn evidence into decoder inputs exist. [@ANCHOR: evidence_to_symbol_values_windowed] is the production
one, with the window half-width as a parameter: signed value `v1 - v0`; amplitude the window mean of the larger tone
minus the window mean of the smaller; noise the square root of the summed variances of the two; amplitude and noise
floored at 1e-9. The window is the symbol plus `half_width` neighbours each side, clamped at both ends.
[@ANCHOR: evidence_to_symbol_values_windowed_clean_reference] is the alternative that takes the noise from the
impossible-tone reference instead (population standard deviation of the 2 x window values). It measured worse at low
SNR and is kept only for the diagnostic tests that record that result.

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

`[@ANCHOR: levinson_durbin_fixed_from_integer_r]` is the production entry point for a caller that already holds the autocorrelation as
integers (the Q8.23 output of `autocorrelate_fixed`, after white-noise correction, which the caller must have applied). It
normalizes with `r0_normalize_fixed`, runs the shared recursion, and returns both the `f32` coefficients (the established output
boundary) and the integer Q8.23 coefficients, so a caller that wants to stay in fixed point (`EncoderFixed` feeds them straight
into `lpc_energy_fixed`, `apply_bw_gamma_fixed` and `lpc_to_lsp_from_integer_ak`) simply uses the second element. Its integer-only
sibling `levinson_durbin_q23_from_integer_r` returns just the Q8.23 coefficients and builds without floating point.

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
