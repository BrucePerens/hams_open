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
