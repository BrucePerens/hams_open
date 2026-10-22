# Story: WSPR transmit and receive (ham_digital_modes)

WSPR (Weak Signal Propagation Reporter) is a beacon mode: a station sends one tiny message, "callsign, 4-character
grid square, power in dBm" (for example `K6BP CM87 30`), once per two-minute slot, and receivers all over the world
report having heard it. The crate implements the standard "Type 1" message only. Compound callsigns such as
`PJ4/K1ABC`, and Type 3 messages with a 6-character grid, are refused rather than guessed at. The protocol facts
(bit layout, polynomials, sync vector, symbol timing) are public and were checked against the `wsprsim` reference
encoder; the decoder is an independent implementation.

## Sending a beacon

**The message.** The callsign is squeezed into 28 bits. [@ANCHOR: pack_call] accepts one or two prefix characters,
one digit, and up to three suffix letters (`K6BP`, `W1AW`, `KA9GRZ`), in either case; it returns nothing for an empty
string, one longer than six characters, or one whose digit is not in the second or third position. The grid and power
go into 22 bits: [@ANCHOR: pack_grid4_power] takes exactly four characters, two Maidenhead field letters `A`-`R`
followed by two digits (case is ignored), and returns nothing for anything else, so a bad grid such as `99EN` or
`SS00` is refused instead of silently turning into a different grid. The power is added to the packed value after a
bias of 64, and the result must fit in 22 bits. Callers must pass a power of 0 to 60 dBm: the packer does not
range-check the power itself, and a power outside the field's 0-127 biased range spills into the grid bits.
The receiving side reverses both steps: [@ANCHOR: unpack_call] turns a 28-bit value back into the callsign (one
implicit leading space and trailing padding are removed, and a value no real callsign produces gives nothing), and
[@ANCHOR: unpack_grid4_power] turns the 22-bit value back into the grid and power, refusing a power outside 0-60 dBm
or a grid outside `AA00`-`RR99`. That power check is what usually catches a confident but wrong decode, because a
garbage 22-bit value still produces some grid and power. [@ANCHOR: unpack_wspr_message] glues the two together for
a decoder result: bit `i` of the 128-bit value is the decision at trellis depth `i`, the first 28 bits are the
callsign (most significant first) and the next 22 the grid and power; it returns `(callsign, grid, power)` or nothing.

**The frame.** [@ANCHOR: wspr_encode_symbols] runs the whole transmit chain for a message and returns the 162
channel symbols, each 0-3. It packs the 50 payload bits, convolutionally encodes them, interleaves the coded bits and
merges in the fixed sync vector so that each symbol is `2 * data_bit + sync_bit`. For `K9AN EN50 33` the result
matches the output of the reference `wsprsim` tool symbol for symbol. It returns nothing for any message the packers
refuse.

**The audio.** [@ANCHOR: wspr_modulate] turns symbols into 16-bit mono PCM: each symbol is a tone at
`base_hz + symbol * 1.4648 Hz`, held for one symbol period (8192 samples at 12 kHz, 0.6827 s), with the carrier phase
carried across symbol boundaries so the transmission has no clicks. At 12 kHz a full transmission is 1,327,104
samples, about 110.6 seconds, at full scale amplitude. The symbols are not validated, so a value above 3 simply
produces a higher tone.

**Test recordings.** To exercise a receiver without a radio, the crate can build a WAV file of a transmission with a
known amount of noise added. [@ANCHOR: add_awgn] adds white Gaussian noise to PCM samples at a requested
signal-to-noise ratio in dB, measured over the whole sample-rate bandwidth (not WSJT-X's 2500 Hz convention, so the
number does not match what `wsprd` prints for the same file); the same seed always gives the same noise, and a silent
input is returned unchanged. [@ANCHOR: wspr_encode_wav_bytes_with_noise] encodes a message, adds that noise, and
returns the bytes of a standard 44-byte-header PCM WAV file, or nothing for an out-of-scope message.

## Receiving a beacon

A receiver is given a buffer of mono 16-bit audio that is at least one full transmission long, and has to find the
signal in time and frequency before it can decode it.

**Getting the audio ready.** The search is tuned and tested at 12 kHz. A caller that records at 48 kHz first calls
[@ANCHOR: decimate_4x_box_average], which replaces every group of four samples by their average (dropping an
incomplete trailing group). To size the search, [@ANCHOR: required_window_samples] gives the number of samples a full
162-symbol transmission occupies at a sample rate; a caller must subtract it from its buffer length to get
`max_start_sample`, because no later start can contain a whole transmission.

**Finding the signal.** [@ANCHOR: find_sync] searches the given audio-frequency band (in steps of the 1.4648 Hz tone
spacing, each tried at eight offsets inside the step) and the start offsets from sample 0 to `max_start_sample` (in
steps of one eighth of a symbol), and scores each candidate by how well the pattern of tones matches the known sync
vector. It returns the best candidate (`base_hz`, `start_sample`, `sync_score` from -1 to 1) or nothing when the
audio is shorter than a transmission, when the band is empty, negative, not finite or above half the sample rate, or
when the best score is under `min_sync_score`; callers normally pass `MIN_SYNC_SCORE` (0.4), which sat between the
scores of pure noise and of signals down to -30 dB in measurement. Only the single strongest signal is found, and no
frequency drift is searched.

**Reading the tones.** Given a frequency and start, [@ANCHOR: extract_symbol_evidence] measures the strength of the
four tones in every symbol and returns, for each of the 162 symbols, the strength of the tone that means "data bit 0"
and of the one that means "data bit 1", chosen using the known sync bit. It returns nothing for audio that is too
short or for a frequency that is negative, not finite or above half the sample rate. [@ANCHOR: evidence_to_symbol_values]
turns those strengths into what the decoder wants: for each symbol a signed value (bit 1 strength minus bit 0
strength), and a local estimate of the signal amplitude and of the noise standard deviation. The estimates use a
window of the symbol and 20 neighbours on each side (shorter at the ends), take the amplitude as the mean of the
stronger tone minus the mean of the weaker one, and the noise as the square root of the sum of the two tones'
variances, with a floor of 1e-9 so silence never produces a zero.

**Decoding.** The decoder works on soft values for the 162 coded bits and searches the code's tree with the stack
algorithm. [@ANCHOR: sequential_decode] takes the per-bit values, the per-bit amplitude and noise estimates, and a
limit on how many tree nodes it may expand. On success it returns the 81 decoded bits (as a bit set) and the final
path metric; if the limit is exceeded it returns `GaveUp` with the number of expansions, never a best guess. Reaching
the end of the tree is not proof of a correct decode near the sensitivity limit, so
[@ANCHOR: sequential_decode_with_confidence_gate] additionally rejects a result whose metric is below a caller-given
threshold, returning `LowConfidence` with the rejected bits and metric so the caller can log them
(`MIN_ACCEPTABLE_METRIC`, 0.0, is the suggested threshold, but a caller posting spots to a public database should use
a higher one). [@ANCHOR: decode_from_symbol_evidence] chains it all for one set of evidence: convert, deinterleave the
values and both estimates, and run the gated decoder.

**End to end.** [@ANCHOR: sync_search_and_decode] searches, extracts the evidence at the winning candidate and
decodes it. It returns nothing when no signal clears the sync threshold; otherwise it returns the detected audio
frequency together with the decoder result, which keeps the frequency even when the decode failed (a caller doing
auto-tune wants to know where the signal was). [@ANCHOR: sync_search_and_decode_message] goes one step further and
unpacks the bits into `(callsign, grid, power, base_hz)`. Its failure type, `WsprMessageError`, has the decoder's two
failures plus `UnpackFailed`, for a decode that was confident but does not make a valid Type 1 message;
[@ANCHOR: WsprMessageError::from] converts the gate's `GaveUp` and `LowConfidence` errors into the matching variants
unchanged.
