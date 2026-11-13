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
known amount of noise added. [@ANCHOR: add_awgn] adds white Gaussian noise to PCM (pulse-code modulation) samples at a requested
signal-to-noise ratio in dB, measured over the whole sample-rate bandwidth (not WSJT-X's 2500 Hz convention, so the
number does not match what `wsprd` prints for the same file); the same seed always gives the same noise, and a silent
input is returned unchanged. [@ANCHOR: wspr_encode_wav_bytes_with_noise] encodes a message, adds that noise, and
returns the bytes of a standard 44-byte-header PCM WAV file, or nothing for an out-of-scope message.

## Receiving a beacon

A receiver is given a buffer of mono 16-bit audio that is at least one full transmission long, and has to find the
signal in time and frequency before it can decode it.

**Getting the audio ready.** The search is tuned and tested at 12 kHz, so a caller that records at 48 kHz decimates first, and sizes the search with the window helper below.

### `decimate_4x_box_average` [@ANCHOR: decimate_4x_box_average]

Converts 48 kHz mono audio to WSPR's native 12 kHz by replacing every group of four samples with their average. It is a crude low-pass, not a proper decimating filter, kept because every threshold in this module was measured at 12 kHz and decimating first keeps production audio on that tested path. Called by the relay's digital-mode pipeline before `find_sync`.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]`, borrowed | Mono PCM at four times the target rate. |

**Returns:** `Vec<i16>` of `samples.len() / 4` values (integer division of the sum by four, so each is rounded toward zero); an incomplete trailing group of one to three samples is dropped, not padded. **Errors:** none. **Special concerns:** pure and allocation-only, no locks; the aliasing a box filter lets through has not been measured on real captures (to-do if real recordings show it). **Signature:** `pub fn decimate_4x_box_average(samples: &[i16]) -> Vec<i16>`. **See also:** the test tagged `decimate_4x_box_average`.

### `required_window_samples` [@ANCHOR: required_window_samples]

Says how many samples one complete 162-symbol transmission occupies at a given sample rate. A caller subtracts it from the length of its buffer to get `max_start_sample` for `find_sync`, because a later start cannot contain a whole transmission; without it the search wastes an order of magnitude more transform work on offsets that cannot hold a signal.

| Argument | Type | Meaning |
|---|---|---|
| `sample_rate` | `u32` | Samples per second of the audio. |

**Returns:** `usize`, `162 * samples_per_symbol(sample_rate)`: 1,327,104 at 12 kHz, 162 x 5461 at 8 kHz, and 0 for a rate of 0. **Errors:** none. **Special concerns:** pure arithmetic; no I/O or locks. **Signature:** `pub fn required_window_samples(sample_rate: u32) -> usize`. **See also:** the test tagged `required_window_samples`, `find_sync`.

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

**End to end.** Two functions chain the search, evidence and decode; they differ in whether they unpack the bits into text.

### `sync_search_and_decode` [@ANCHOR: sync_search_and_decode]

Runs `find_sync`, then `extract_symbol_evidence` at the winning candidate, then `decode_from_symbol_evidence`. It keeps the detected audio frequency even when the decode failed, because an auto-tune caller wants to know where the signal was.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]`, borrowed | Mono PCM at 12 kHz (decimate first). |
| `sample_rate` | `u32` | Samples per second. |
| `freq_lo_hz`, `freq_hi_hz` | `f64` | Audio band searched, in hertz; see `find_sync` for the rejected values. |
| `max_start_sample` | `usize` | Last start offset tried; `samples.len()` minus `required_window_samples`. |
| `min_sync_score` | `f64` | Sync threshold, normally `MIN_SYNC_SCORE` (0.4). |
| `max_cycles` | `u64` | Tree-node budget for the sequential decoder. |
| `min_acceptable_metric` | `f64` | Confidence gate, at least `MIN_ACCEPTABLE_METRIC` (0.0); higher for public spots. |

**Returns:** `Option<(f64, Result<u128, ConfidenceGateError>)>`: `None` when no signal clears the sync threshold; otherwise the detected `base_hz` and either the 81 decoded bits (in a `u128`) or the gate's `GaveUp` or `LowConfidence` error. **Special concerns:** pure and CPU-bound (it can run for seconds on a full buffer, so run it off the audio thread); no I/O, no locks. **See also:** the test tagged `sync_search_and_decode`.

### `sync_search_and_decode_message` [@ANCHOR: sync_search_and_decode_message]

The call a real receiver makes: `sync_search_and_decode` followed by `unpack_wspr_message`. It takes the same eight arguments in the same order and meaning.

**Returns:** `Option<Result<(String, String, i32, f64), WsprMessageError>>`: `None` when no signal was found; `Ok((callsign, grid, power_dbm, base_hz))` for a decoded Type 1 message; else a `WsprMessageError`. **Errors:** `GaveUp { cycles }` (node budget exhausted), `LowConfidence { bits, metric }` (path metric under the gate) and `UnpackFailed { bits }` (confident decode that is not a valid Type 1 message, for example a power outside 0 to 60 dBm). **Special concerns:** as `sync_search_and_decode`; the frequency is returned only on success, so an auto-tune caller wanting it after a failure should use the lower-level function. **See also:** the test tagged `sync_search_and_decode_message`.

### `WsprMessageError::from` [@ANCHOR: WsprMessageError::from]

The `From<ConfidenceGateError>` conversion that lets `sync_search_and_decode_message` use `?`-style propagation. `WsprMessageError` is the decoder's two failures plus `UnpackFailed`; this conversion maps `GaveUp { cycles }` and `LowConfidence { bits, metric }` to the variants of the same names with their data unchanged.

**Call interface:** `fn from(e: ConfidenceGateError) -> WsprMessageError`; `e` is moved in. **Returns:** the matching variant. **Errors and concerns:** none; total, pure, no allocation. **Data:** `WsprMessageError` has the variants `GaveUp { cycles: u64 }`, `LowConfidence { bits: u128, metric: f64 }` and `UnpackFailed { bits: u128 }`. **See also:** the test tagged `WsprMessageError::from`, which checks that pure noise never produces a confident `Ok`.
