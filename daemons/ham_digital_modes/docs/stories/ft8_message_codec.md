# Story: FT8 messages, tones and audio (ham_digital_modes)

FT8 is a 15-second slotted mode. The operator gets message text packed into the 77-bit payload and 79 tones, a
waveform to transmit, and decoded messages with signal-to-noise and audio frequency for each. Packing,
error-correction coding and decoding come from the vendored `ft8_lib` through a small C shim.

## Sending

### `encode_message` [@ANCHOR: encode_message]

`encode_message` packs a text message (for example `CQ K1ABC FN42`) and returns the 79 tone indices, each 0 to
7. It returns `None` when the text cannot be packed as a valid FT8 message (such as an invalid callsign) or
contains an interior NUL byte. It produces no audio.
*(Reference: `src/ft8.rs`)*

### `synthesize_waveform_with_pulse` [@ANCHOR: synthesize_waveform_with_pulse]

`synthesize_waveform_with_pulse` is what makes the audio behind `synthesize_waveform`. For 79 tones at 0.160 s
each it builds continuous-phase FSK: the frequency offset is the sum of one pulse per symbol (only the
current and the two adjacent symbols can contribute), the phase is that offset integrated sample by sample,
and the carrier is `cos(2*pi*base_freq*t + phase)`. The first and last 20 ms ramp the amplitude with a raised
cosine so the signal does not key with a hard edge. The result is f32 samples between -1 and 1, about 12.64 s
long. The pulse shape is a parameter so a test can compare the Gaussian pulse with an unfiltered one.
*(Reference: `src/ft8.rs`)*

## Receiving

### `Ft8Decoder::new` [@ANCHOR: Ft8Decoder::new]

`Ft8Decoder::new` creates a decoder for a given sample rate (12000 Hz is standard) and a frequency window
(200 to 3000 Hz is WSJT-X's default). It returns `None` if the library cannot create a session, and also when
the sample rate yields a zero-length analysis block, such as a rate of 0 Hz, which would otherwise make
`feed` loop forever.
*(Reference: `src/ft8.rs`)*

### `Ft8Decoder::feed` [@ANCHOR: Ft8Decoder::feed]

`Ft8Decoder::feed` accepts audio in chunks of any size, passes each complete analysis block to the waterfall,
and keeps the remainder. It does not decode.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `Ft8Decoder` | Holds the C session, the unconsumed remainder `pending` and the block size. |
| `samples` | `&[f32]`, borrowed | Mono audio, nominally -1 to 1, at the sample rate given to `new`. |

**Returns:** nothing. **Errors:** none; whole analysis blocks go straight to the C waterfall through `ft8_session_process`, and the remainder (less than one block) stays in `pending`. **Special concerns:** it runs on the calling thread and does the transform work for each block, so call it from the audio-processing task rather than a UI thread. The session is `Send` but not `Sync`; one thread at a time. **See also:** `Ft8Decoder::decode`, `Ft8Decoder::reset`.

*(Reference: `src/ft8.rs`)*

### `Ft8Decoder::decode` [@ANCHOR: Ft8Decoder::decode]

`Ft8Decoder::decode` is called once at the end of a slot. It decodes up to 50 candidates and returns
(message text, SNR estimate, audio frequency in Hz) for each. It does not clear the waterfall; `reset` does that
before the next slot.
*(Reference: `src/ft8.rs`)*

### `Ft8Decoder::decode_timed` [@ANCHOR: Ft8Decoder::decode_timed]

`Ft8Decoder::decode_timed` is `decode` with a fourth value per message: the signal's start in seconds from the
first sample fed since the last `reset` (slightly negative is possible). With audio fed from the slot boundary,
the DT that WSJT-X reports is this value minus 0.5 s.
*(Reference: `src/ft8.rs`)*
