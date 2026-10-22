# Story: P25 full-rate voice, from the air to audio and back (ham_digital_modes)

This is the path one 20 ms burst of P25 Phase 1 voice (the TIA-102.BABA IMBE codec; the DVSI chip's `RATET(27)` setting is a
different, proprietary codec and is only kept as an option for the chip-comparison tools) takes through the float AMBE code of
this crate. A frame is 144 bits, which the channel carries as 72 two-bit symbols. 88 of the bits are speech parameters and 56 are
error correction. Receiving undoes the transmitter's steps in reverse order; the sections below follow the receive direction
first and then the transmit direction, and each one names the function that does the work.

## Receiving: symbols to code vectors to audio

1. **Un-interleave.** The transmitter spreads the 144 bits over the 72 symbols so that a short burst of channel errors lands in
   several different error-correction words instead of one (Annex H of the standard keeps any two bits of one word at least three
   symbols apart). `deinterleave_from_dibit_symbols` takes the 72 received `(bit1, bit0)` symbols and writes every one of the 144
   positions of the Annex H table back into the eight code vectors `c_hat_0..c_hat_7`. It cannot fail: every output bit is written
   exactly once.
   *(Reference: `src/ambe/float/tia_102_baba/interleave.rs` -> `[@ANCHOR: deinterleave_from_dibit_symbols]`)*

2. **Remove the scrambling.** To make a damaged `c_hat_0` visible instead of silently wrong, the transmitter XORs the other code
   vectors with a pseudo-random bit pattern that depends on `u_hat_0`. The pattern is a 16-bit linear congruential sequence
   (Eq. 84-85): the seed is `16 * u_hat_0` and each next value is `173 * previous + 13849` modulo 65536, for 115 values.
   `pseudo_random_sequence` produces that sequence for a 12-bit `u_hat_0`.
   *(Reference: `src/ambe/float/tia_102_baba/modulation.rs` -> `[@ANCHOR: pseudo_random_sequence]`)*
   `modulation_vectors` turns it into the eight masks `m_hat_0..m_hat_7` (Eq. 86-93): the top bit of each of the values 1..114 is
   cut into consecutive slices of 23 bits for the three Golay vectors and 15 bits for the three Hamming vectors. `m_hat_0` and
   `m_hat_7` are zero, because `u_hat_0` must stay readable to seed the rest and `u_hat_7` has no error correction to desynchronise.
   *(Reference: `src/ambe/float/tia_102_baba/modulation.rs` -> `[@ANCHOR: modulation_vectors]`)*

3. **Error-correct.** The code vectors `c_hat_0..c_hat_3` are `[23,12]` Golay codewords and `c_hat_4..c_hat_6` are `[15,11]` Hamming
   codewords (Eq. 81-83); both are systematic, meaning the data bits come first and the parity bits follow. `c_hat_7` is seven
   bits of unprotected data.
   `golay_encode` takes the low 12 bits of its argument and returns the 23-bit codeword (12 data bits then 11 parity bits, the
   parity being the XOR of the matrix rows selected by the set data bits). `hamming_encode` does the same for 11 data bits and a
   4-bit parity. `golay_decode` and `hamming_decode` are minimum-distance decoders: they compare the received word with every
   possible codeword (4096 and 2048 of them) and return the data of the closest one together with the number of bits that
   differed. That count is the number of corrected errors, which the decoder uses as its link-quality signal. The Golay code
   recovers any word with up to three bit errors and the Hamming code any with one. Beyond that the result is still the nearest
   codeword but is no longer guaranteed to be what was sent; ties go to the smallest data value because the search keeps the first
   best match. D-STAR frames use the same Golay encoder and decoder.
   *(Reference: `src/ambe/general/fec.rs` -> `[@ANCHOR: golay_encode]`, `[@ANCHOR: hamming_encode]`, `[@ANCHOR: golay_decode]`, `[@ANCHOR: hamming_decode]`)*

   `decode_code_vectors` is the whole of steps 2 and 3: it Golay-decodes `c_hat_0` first, uses the corrected `u_hat_0` to build the
   masks, XORs them off `c_hat_1..c_hat_6`, decodes those, and passes `c_hat_7` through. It returns the eight bit vectors
   `u_hat_0..u_hat_7` and the seven corrected-error counts `epsilon_0..epsilon_6` (there is no `epsilon_7`). Keying the masks off
   the corrected `u_hat_0` is what makes an uncorrectable `c_hat_0` turn into noise in the other vectors.
   *(Reference: `src/ambe/float/tia_102_baba/mod.rs` -> `[@ANCHOR: ambe_mod:decode_code_vectors]`)*

4. **Measure how bad the frame was.** `estimate_errors` (Eq. 95-96) adds the seven counts into the frame total `epsilon_T`, updates
   the running error rate `epsilon_R = 0.95 * previous + 0.000365 * epsilon_T`, and keeps `epsilon_0` and `epsilon_4` aside because
   later thresholds look at them on their own. The decoder uses the result to decide between decoding, repeating the previous
   frame (section 7.7) and muting (section 7.8, `epsilon_R > 0.0875`).
   *(Reference: `src/ambe/float/tia_102_baba/error_estimation.rs` -> `[@ANCHOR: estimate_errors]`)*

5. **Unscramble the 88 bits into parameters.** The speech parameters are not stored one after another: Fig. 22 of the standard
   arranges them so the most error-sensitive bits sit in the best-protected vectors `u_hat_0..u_hat_3` and the least sensitive
   ones in the weakly protected tail. For the gain and higher-order coefficients the arrangement is a raster scan: the values are
   drawn as columns as tall as their bit allocation, and the bits are read off from the most significant level downward, left to
   right, skipping columns that are too short for the level.
   `raster_scan_bits` does that scan for the transmit side and returns the flat bit sequence.
   *(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: raster_scan_bits]`)*
   `prioritize_bits` assembles the whole 88-bit stream in the standard's nine steps (the top six bits of `b0`, the top three of
   `b2`, the first 39 scanned bits, all of `b1`, two more `b2` bits, the rest of the scan, the last `b2` bit, the low two bits of
   `b0`, and finally the alternating synchronisation bit) and cuts it into `u_hat_0..u_hat_7` of 12, 12, 12, 12, 11, 11, 11 and 7
   bits. It returns `None` unless the inputs add up to exactly 88 bits.
   *(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: prioritize_bits]`)*
   On receive there is a chicken-and-egg problem: unscrambling needs the number of V/UV bands and the column widths, which depend
   on the pitch `b_hat_0`, which is itself scattered across the stream. `extract_fundamental_frequency_quantizer` breaks it: the
   top six bits of `b_hat_0` are always the first six bits of `u_hat_0`, and the low two bits are always bits 2 and 1 of `u_hat_7`
   (the last four stream bits are always the last bit of `b_hat_2`, those two pitch bits and the sync bit, whatever the frame
   shape), so it reads `b_hat_0` out of `u_hat_0` and `u_hat_7` alone.
   *(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: extract_fundamental_frequency_quantizer]`)*
   Once the pitch is known, `deprioritize_bits` inverts `prioritize_bits`: it flattens the eight vectors back to 88 bits, reads the
   fields in the same order using the now-known `k_hat` and column widths, un-scans the columns, and returns every quantizer value
   plus the sync bit. It returns `None` if the lengths do not work out to exactly 88 bits.
   *(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: deprioritize_bits]`)*

6. **Decode, then speak.** `DecoderState` is the receiver. A new one starts from the standard's Annex A values: 30 harmonics in
   the previous frame, all with amplitude 1 (not silence), an error rate of zero, the standard's own wire layer and pitch map, and
   fresh synthesis state.
   *(Reference: `src/ambe/float/tia_102_baba/decode.rs` -> `[@ANCHOR: ambe:decoder_state_new]`)*
   `Default` gives exactly that state.
   *(Reference: `src/ambe/float/tia_102_baba/decode.rs` -> `[@ANCHOR: ambe:decoder_state_default]`)*
   `decode_parameters` runs everything above up to, but not including, synthesis, for one frame's eight code vectors. It returns
   `Mute` when the error rate exceeds the mute threshold (checked first), `Repeat` when `b_hat_0` is above 207 or the Eq. 97-98
   repeat test fires, and otherwise `Decoded` with the pitch, `L`, `K`, the unscrambled quantizer values, per-harmonic voicing and
   the reconstructed spectral amplitudes. It returns `None` only when a table lookup or length check fails inside the pipeline.
   Splitting it from synthesis lets a caller compare the recovered parameters, not just the audio.
   *(Reference: `src/ambe/float/tia_102_baba/decode.rs` -> `[@ANCHOR: ambe:decode_parameters]`)*
   `decode_frame` is the operator-facing call: eight code vectors in, 160 samples (20 ms at 8 kHz) of audio out. A decoded frame
   is synthesised and becomes the new history; a repeat re-synthesises the last good frame; a mute produces comfort noise. It
   returns `None` only if the very first frame of a stream needs a repeat (nothing to repeat yet), or if decoding itself fails.
   *(Reference: `src/ambe/float/tia_102_baba/decode.rs` -> `[@ANCHOR: ambe:decode_frame]`)*
   `synthesize_comfort_frame` is the mute branch: section 7.8 says to replace speech with noise uniformly distributed on
   `[-5, 5]`. It draws 160 values from its own copy of the Eq. 117 noise generator (kept separate so a muted frame never disturbs
   the noise sequence that normal synthesis depends on) and scales each to that interval.
   *(Reference: `src/ambe/float/tia_102_baba/synthesis.rs` -> `[@ANCHOR: ambe:synthesize_comfort_frame]`)*

## Transmitting: audio to code vectors

`encode_frame` runs the whole transmit pipeline for one 20 ms frame. Its inputs are the analysis of the speech around the frame
(`RefinementFrame`), the refined pitch `omega0_hat`, the error value of the initial pitch estimate, the previous frame's
`FrameState` and the alternating sync bit. It chooses the voiced/unvoiced bands, estimates the spectral amplitudes, takes the
prediction residual against the previous frame, applies the block DCTs, quantizes, prioritizes the bits, adds Golay/Hamming
parity and modulates, and returns the eight code vectors `c_hat_0..c_hat_7` together with the new `FrameState`. That state holds
the amplitudes the *decoder* will reconstruct (the quantized values run back through dequantization), not the encoder's own raw
estimate, so the next frame is predicted from what the receiver actually has. It returns `None` if the pitch implies a harmonic
count outside 9 to 56 or the bit count does not come to 88. The code vectors are then interleaved into the 72 symbols for the
channel.
*(Reference: `src/ambe/float/tia_102_baba/mod.rs` -> `[@ANCHOR: encode_frame]`)*
