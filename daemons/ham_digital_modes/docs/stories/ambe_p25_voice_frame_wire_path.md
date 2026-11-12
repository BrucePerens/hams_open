# Story: P25 full-rate voice, from the air to audio and back (ham_digital_modes)

This is the path one 20 ms burst of P25 Phase 1 voice (the TIA-102.BABA IMBE codec; the DVSI chip's `RATET(27)` setting is a
different, proprietary codec and is only kept as an option for the chip-comparison tools) takes through the float AMBE code of
this crate. A frame is 144 bits, which the channel carries as 72 two-bit symbols. 88 of the bits are speech parameters and 56 are
error correction. Receiving undoes the transmitter's steps in reverse order; the sections below follow the receive direction
first and then the transmit direction, and each one names the function that does the work.

## Receiving: symbols to code vectors to audio

1. **Un-interleave.** The transmitter spreads the 144 bits over the 72 symbols so that a short burst of channel errors lands in
   several different error-correction words instead of one (Annex H of the standard keeps any two bits of one word at least three
   symbols apart). `deinterleave_from_dibit_symbols` (below) takes the 72 received symbols and puts every bit back into the eight code vectors `c_hat_0..c_hat_7`.

### `deinterleave_from_dibit_symbols` [@ANCHOR: deinterleave_from_dibit_symbols]

Receive-side inverse of the Annex H interleave: it takes the 72 received two-bit symbols and writes every one of the 144 bit positions of the `BIT_FRAME_FORMAT` table back into the eight code vectors `c_hat_0..c_hat_7`. It is the first step of decoding a P25 voice frame and is called by the P25 receive path with the symbols the channel layer produced.

| Argument | Type | Meaning |
|---|---|---|
| `symbols` | `[(bool, bool); 72]`, by value | Received dibits in air order, each as `(bit1, bit0)`, `bit1` being the more significant bit of the symbol. |

**Returns:** `[u32; 8]`, the code vectors `c_hat_0..c_hat_7`; the low 23 bits hold the Golay vectors 0 to 3, the low 15 bits the Hamming vectors 4 to 6 and the low 7 bits vector 7, each most significant bit first. **Errors:** none; every output bit is written exactly once. **Special concerns:** pure, constant time, no locks, and no error correction happens here (a bit error stays where the channel put it). **Signature:** `pub fn deinterleave_from_dibit_symbols(symbols: [(bool, bool); 72]) -> [u32; 8]`. **See also:** its inverse in the same file, and the test `deinterleave_is_the_exact_inverse_of_interleave_for_several_real_bit_patterns`.

2. **Remove the scrambling.** To make a damaged `c_hat_0` visible instead of silently wrong, the transmitter XORs the other code
   vectors with a pseudo-random bit pattern that depends on `u_hat_0`. The pattern is a 16-bit linear congruential sequence
   (Eq. 84-85): the seed is `16 * u_hat_0` and each next value is `173 * previous + 13849` modulo 65536, for 115 values.
   The two functions below do it.

### `pseudo_random_sequence` [@ANCHOR: pseudo_random_sequence]

Produces the 115-value scrambling sequence of Eq. 84-85 from the 12-bit `u_hat_0`: `pr[0] = 16 * u_hat_0`, then `pr[n] = (173 * pr[n-1] + 13849) mod 65536` for n = 1 to 114. Both the transmitter (to scramble) and the receiver (to unscramble) call it, always seeded with the corrected `u_hat_0`.

| Argument | Type | Meaning |
|---|---|---|
| `u0` | `u32` | The 12-bit `u_hat_0`, 0 to 4095; larger values are not range-checked and their seed wraps when cast to 16 bits. |

**Returns:** `[u16; 115]`. **Errors:** none. **Special concerns:** pure, constant time, not cryptographic (a published scrambler that exists only to make an uncorrectable `c_hat_0` visible). **Signature:** `pub fn pseudo_random_sequence(u0: u32) -> [u16; 115]`. **See also:** `modulation_vectors`, and test `pseudo_random_sequence_matches_eq84_85_at_hand_computed_values`.

### `modulation_vectors` [@ANCHOR: modulation_vectors]

Turns the sequence into the eight masks `m_hat_0..m_hat_7` (Eq. 86-93) that are XORed onto the code vectors. The top bit of each of the values 1 to 114 is cut into consecutive slices of 23 bits for the three masks that go onto Golay vectors 1 to 3 and 15 bits for the three that go onto Hamming vectors 4 to 6. `m_hat_0` and `m_hat_7` are zero: `u_hat_0` has to stay readable to seed the rest and `u_hat_7` has no error correction to desynchronise.

| Argument | Type | Meaning |
|---|---|---|
| `u0` | `u32` | The 12-bit `u_hat_0` (the corrected value on receive). |

**Returns:** `[u32; 8]` masks, index `i` for `c_hat_i`. **Errors:** none. **Special concerns:** pure; calls `pseudo_random_sequence`. **Signature:** `pub fn modulation_vectors(u0: u32) -> [u32; 8]`. **See also:** test `modulation_vectors_m0_and_m7_are_always_zero`, and `decode_code_vectors`, which applies the masks.

3. **Error-correct.** The code vectors `c_hat_0..c_hat_3` are `[23,12]` Golay codewords and `c_hat_4..c_hat_6` are `[15,11]` Hamming
   codewords (Eq. 81-83); both are systematic, meaning the data bits come first and the parity bits follow. `c_hat_7` is seven
   bits of unprotected data.
   The four codec functions are documented in their own entries below. D-STAR frames use the same Golay encoder and decoder.

### `golay_encode` [@ANCHOR: golay_encode]

Encodes 12 data bits as a 23-bit systematic Golay codeword: the data bits first, then 11 parity bits, each the XOR of the matrix rows (`GOLAY_PARITY`) selected by the set data bits.

| Argument | Type | Meaning |
|---|---|---|
| `data` | `u16` | Data in the low 12 bits; higher bits are masked off. |

**Returns:** `u32` codeword (23 bits used). **Errors:** none. **Special concerns:** pure, 12 table steps. **Signature:** `pub fn golay_encode(data: u16) -> u32`. **See also:** `golay_decode`, test `golay_weight_distribution_matches_the_known_enumerator`.

### `hamming_encode` [@ANCHOR: hamming_encode]

The same for the `[15,11]` Hamming code: 11 data bits followed by a 4-bit parity from the `HAMMING_PARITY` rows.

| Argument | Type | Meaning |
|---|---|---|
| `data` | `u16` | Data in the low 11 bits; higher bits are masked off. |

**Returns:** `u16` codeword (15 bits used). **Errors:** none. **Special concerns:** pure. **Signature:** `pub fn hamming_encode(data: u16) -> u16`. **See also:** `hamming_decode`, test `hamming_weight_distribution_matches_the_known_enumerator`.

### `golay_decode` [@ANCHOR: golay_decode]

Minimum-distance decoder: it compares the received word with every one of the 4096 codewords and returns the data of the closest one with the number of bits that differed. That count is the number of corrected errors, which the decoder uses as its link-quality signal. Any pattern of up to three bit errors is corrected; beyond that the result is still the nearest codeword but is no longer guaranteed to be what was sent. Ties go to the smallest data value, because the search keeps the first best match.

| Argument | Type | Meaning |
|---|---|---|
| `received` | `u32` | Received word in the low 23 bits; higher bits are masked off. |

**Returns:** `(u16, u32)`: the 12 corrected data bits and the Hamming distance moved. **Errors:** none; it cannot report an uncorrectable word except through a distance above 3. **Special concerns:** pure; a brute-force search of 4096 candidates per call, which is cheap per frame but not free (several decodes per P25 frame). **Signature:** `pub fn golay_decode(received: u32) -> (u16, u32)`. **See also:** test `golay_decode_recovers_the_original_from_up_to_three_bit_errors`.

### `hamming_decode` [@ANCHOR: hamming_decode]

The same search over the 2048 codewords of the `[15,11]` code. One bit error is always corrected; with two or more the answer is the nearest codeword, not necessarily the one sent, and ties go to the smallest data value.

| Argument | Type | Meaning |
|---|---|---|
| `received` | `u16` | Received word in the low 15 bits; higher bits are masked off. |

**Returns:** `(u16, u32)`: the 11 corrected data bits and the number of bits that differed. **Errors:** none. **Special concerns:** pure; 2048 candidates per call. **Signature:** `pub fn hamming_decode(received: u16) -> (u16, u32)`. **See also:** test `hamming_decode_recovers_the_original_from_a_single_bit_error`.

### `decode_code_vectors` [@ANCHOR: ambe_mod:decode_code_vectors]

Steps 2 and 3 together: it Golay-decodes `c_hat_0` first, uses the corrected `u_hat_0` to build the masks, XORs them off `c_hat_1..c_hat_6`, decodes those and passes `c_hat_7` through unchanged. Keying the masks off the corrected `u_hat_0` is what makes an uncorrectable `c_hat_0` turn into noise in the other vectors, which the error estimate then reports.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `[u32; 8]`, by value | The code vectors from `deinterleave_from_dibit_symbols`. |

**Returns:** `([u32; 8], [u32; 7])`: the data vectors `u_hat_0..u_hat_7` and the corrected-error counts `epsilon_0..epsilon_6` (there is no `epsilon_7`). **Errors:** none. **Special concerns:** pure; four Golay and three Hamming decodes per call. **Signature:** `pub fn decode_code_vectors(c: [u32; 8]) -> ([u32; 8], [u32; 7])`. **See also:** `golay_decode`, `hamming_decode`, `modulation_vectors`, and test `encode_code_vectors_is_modulated_textbook_fec_and_decode_code_vectors_inverts_it`.

4. **Measure how bad the frame was.** `estimate_errors` (below) turns the seven counts into the numbers the decoder's repeat and mute decisions use.

### `estimate_errors` [@ANCHOR: estimate_errors]

Implements Eq. 95-96: it adds the seven corrected-error counts into the frame total `epsilon_T`, updates the running error rate `epsilon_R = 0.95 * previous + 0.000365 * epsilon_T`, and keeps `epsilon_0` and `epsilon_4` aside because later thresholds look at them on their own. The decoder uses the result to decide between decoding, repeating the previous frame (section 7.7) and muting (section 7.8, `epsilon_R > 0.0875`).

| Argument | Type | Meaning |
|---|---|---|
| `corrected_error_counts` | `&[u32; 7]`, borrowed | `epsilon_0..epsilon_6` from `decode_code_vectors`. |
| `previous_rate` | `f64` | The previous frame's `epsilon_R`; 0.0 at the start of a stream. |

**Returns:** `FrameErrors { total: u32, rate: f64, golay_init: u32, hamming_init: u32 }`: `total` is `epsilon_T`, `rate` the new `epsilon_R`, `golay_init` is `epsilon_0` and `hamming_init` is `epsilon_4`. **Errors:** none. **Special concerns:** pure; the caller carries `rate` forward as state between frames, one decoder per stream. **Signature:** `pub fn estimate_errors(corrected_error_counts: &[u32; 7], previous_rate: f64) -> FrameErrors`. **See also:** test `estimate_errors_matches_eq95_96_at_a_hand_computed_value`.

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

### `encode_frame` [@ANCHOR: encode_frame]

Runs the whole transmit pipeline for one 20 ms frame. It chooses the voiced/unvoiced bands, estimates the spectral amplitudes, takes the prediction residual against the previous frame, applies the block DCTs, quantizes, prioritizes the bits, adds Golay/Hamming parity and modulates. The code vectors are then interleaved into the 72 symbols for the channel by the caller.

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame`, borrowed | Analysis of the speech around the frame. |
| `omega0_hat` | `f64` | The refined pitch (fundamental) in radians per sample. |
| `initial_pitch_error` | `f64` | The error value of the initial pitch estimate. |
| `previous_state` | `&FrameState`, borrowed | The state returned for the previous frame; the initial state for the first frame of a stream. |
| `sync_bit` | `bool` | The alternating synchronisation bit appended at the end of the 88 bits. |

**Returns:** `Option<([u32; 8], FrameState)>`: the code vectors `c_hat_0..c_hat_7` and the new state. The state holds the amplitudes the *decoder* will reconstruct (the quantized values run back through dequantization), not the encoder's raw estimate, so the next frame is predicted from what the receiver actually has. **Errors:** `None` if the pitch implies a harmonic count outside 9 to 56, or the bit count does not come to 88. **Special concerns:** pure and CPU-bound; one encoder per stream because the returned state must be fed to the next call in order. It produces bits only; sending them is the job of code that must pass the relay's transmit lock, band check and transmit gate. **Signature:** `pub fn encode_frame(frame: &RefinementFrame, omega0_hat: f64, initial_pitch_error: f64, previous_state: &FrameState, sync_bit: bool) -> Option<([u32; 8], FrameState)>`. **See also:** test `encode_frame_produces_a_full_frame_from_a_real_synthetic_harmonic_signal`.
