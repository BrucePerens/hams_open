# Story: RTTY text to audio and back (ham_digital_modes)

An operator gets standard amateur radio teletype: 45.45 baud, 170 Hz shift, MARK at the configured audio tone
(2125 Hz by default) and SPACE 170 Hz above it, 5-bit Baudot (ITA2 with the US-TTY figures variant), one
SPACE start bit, five data bits least significant first, and a 1.5-bit MARK stop bit.

## Sending

### `char_to_baudot` [@ANCHOR: char_to_baudot]

`char_to_baudot` maps one character, case-insensitively, to its 5-bit code and says whether it needs the
figures shift. Anything with no Baudot code (the table is 5-bit limited by design) yields nothing.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `char` | Character to encode; ASCII letters are upper-cased first. |

**Returns:** `Option<(u8, bool)>`: the 5-bit code (0 to 31) and `false` for the letters shift or `true` for figures; `None` when neither shift table holds the character. **Errors:** none. **Special concerns:** private, pure lookup in the two 32-entry tables (letters is searched first, so a character in both tables is sent as a letter). **See also:** `text_to_framed_bits`, which calls it, and the tests tagged `char_to_baudot`.

*(Reference: `src/rtty.rs`)*

### `text_to_framed_bits` [@ANCHOR: text_to_framed_bits]

`text_to_framed_bits` builds the framed bit sequence for a message (true is MARK, false is SPACE). It inserts a
LTRS or FIGS shift character only when the shift state actually changes, starting in letters, and skips
characters that have no code.

**Call interface:** private `fn text_to_framed_bits(text: &str) -> Vec<bool>`; `text` is borrowed. Each character becomes a start bit (`false`), five data bits least significant first and the stop bit (`true`), with the stop stretch applied later by the modulator. **Returns:** one `bool` per bit period; empty for text with no encodable characters. **Errors:** none. **Special concerns:** pure and allocation-only; the shift state begins in letters and persists through the message, so a message that ends in figures does not send a closing LTRS. **See also:** `char_to_baudot`, `rtty_modulate`, and the tests tagged `text_to_framed_bits`.

*(Reference: `src/rtty.rs`)*

### `rtty_modulate` [@ANCHOR: rtty_modulate]

`rtty_modulate` turns text into 16-bit mono PCM at 0.9 of full scale as continuous-phase FSK, so there is no
phase jump at MARK/SPACE changes, and stretches every stop bit to 1.5 bit periods.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str`, borrowed | Message; characters without a Baudot code are skipped. |
| `mark_hz` | `f64` | MARK tone in hertz (2125 is the usual choice); SPACE is `mark_hz + 170`. |
| `sample_rate` | `u32` | Output samples per second. |

**Returns:** `Vec<i16>`, mono PCM, peak 0.9 of full scale, with continuous phase across tone changes. **Errors:** none. **Special concerns:** pure and synchronous. It only builds audio; sending it to a radio is the caller's job and has to pass the relay's transmit lock, band check and transmit gate. **Signature:** `pub fn rtty_modulate(text: &str, mark_hz: f64, sample_rate: u32) -> Vec<i16>`. **See also:** `text_to_framed_bits`, `framed_bits_to_audio` and the tests tagged `rtty_modulate`.

*(Reference: `src/rtty.rs`)*

## Receiving

RTTY is asynchronous, so a decoder has to find character starts itself. A candidate start is accepted when the
audio before it is sustained MARK, the start bit reads SPACE, the stop bit reads MARK, and the five data bits
show a real two-tone signal; the thresholds are on MARK/SPACE energy ratios, so they work at any signal level.
The search also tries nine carrier offsets from -40 to +40 Hz in 10 Hz steps, so a mistuned receiver still
decodes.

### `rtty_demodulate` [@ANCHOR: rtty_demodulate]

`rtty_demodulate` decodes a whole buffer in one call and has no memory between calls: it creates a fresh scan state, runs `rtty_scan` over every sample and returns all characters found, so a character split across two calls is never recovered (use `RttyDecoder` for a stream).

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]`, borrowed | Mono PCM of the complete transmission. |
| `mark_hz` | `f64` | Nominal MARK tone in hertz; offsets of -40 to +40 Hz around it are also tried. |
| `sample_rate` | `u32` | Samples per second; if it gives under one sample per bit the result is empty. |

**Returns:** `String` of decoded text (letters and figures resolved through the shift rules). **Errors:** none. **Special concerns:** pure; the cost is the offset search, nine hypotheses per candidate start. **Signature:** `pub fn rtty_demodulate(samples: &[i16], mark_hz: f64, sample_rate: u32) -> String`. **See also:** `rtty_scan`, `attempt_character`, tests tagged `rtty_demodulate`.
*(Reference: `src/rtty.rs`)*

### `RttyDecoder::new` [@ANCHOR: RttyDecoder::new]

`RttyDecoder::new` builds the streaming decoder with the full offset search; chunks of any size can be fed and
characters arrive as they complete. It holds back a character until enough audio is buffered to confirm it, so
the last character of a burst can appear one character time (about 150 ms) late, on the next feed.

| Argument | Type | Meaning |
|---|---|---|
| `mark_hz` | `f64` | Nominal MARK tone in hertz. |
| `sample_rate` | `u32` | Samples per second of the audio that will be fed. |

**Returns:** an `RttyDecoder` holding an empty buffer, a scan state and a bank of tone correlators for the nine offsets. **Errors:** none; a rate too low for one sample per bit gives a decoder that never outputs anything. **Special concerns:** building the correlator tables costs a few thousand trigonometric calls, so build once per channel, not per chunk. A decoder is used from one task at a time (`feed` takes `&mut self`). **See also:** `TrigBank::new`, `RttyDecoder::feed`, tests tagged `RttyDecoder::new`.

*(Reference: `src/rtty.rs`)*

### `RttyDecoder::new_fixed` [@ANCHOR: RttyDecoder::new_fixed]

`RttyDecoder::new_fixed` is the cheap variant for the wide-band channel scanner: one carrier hypothesis, no
offset search. It tolerates about 25 Hz of mistuning cleanly, so a scanner bank that spaces candidates 40 Hz
apart still finds a signal, and the winner is then promoted to a full decoder.

**Call interface:** `pub fn new_fixed(mark_hz: f64, sample_rate: u32) -> RttyDecoder`; both arguments as for `RttyDecoder::new`. **Returns:** a decoder whose correlator bank holds a single zero-offset hypothesis, so `feed` costs about a ninth of the full decoder. **Errors:** none. **Special concerns:** same threading rule as `new`; with one hypothesis `locked_frequency_offset_hz` is always 0.0. A scanner promotes a winner by building a full `RttyDecoder::new` at the frequency it found. **See also:** `TrigBank::new_with_offsets`, tests tagged `RttyDecoder::new_fixed`.

*(Reference: `src/rtty.rs`)*

### `RttyDecoder::feed` [@ANCHOR: RttyDecoder::feed]

`RttyDecoder::feed` takes the next chunk of audio (`samples: &[i16]`, borrowed, any length) and returns a `String` of the characters completed so far (usually empty). It needs `&mut self` and is not safe to share between tasks without the caller's lock. There are no errors. A sample rate
too low for one sample per bit makes it return nothing without buffering, and already-scanned audio is
dropped once more than ten seconds of it has accumulated.
*(Reference: `src/rtty.rs`)*

### `RttyDecoder::locked_frequency_offset_hz` [@ANCHOR: RttyDecoder::locked_frequency_offset_hz]

`RttyDecoder::locked_frequency_offset_hz` reports the offset of the candidate that decoded the most recent
character, or 0.0 before any character has decoded; for a `new_fixed` decoder it is always 0.0.

**Call interface:** `pub fn locked_frequency_offset_hz(&self) -> f64`; borrows the decoder read-only and has no side effects. **Returns:** the MARK offset in hertz (one of -40, -30, ..., +40) of the correlator that decoded the last character, or 0.0 before any character. A caller doing auto-tune adds it to the nominal MARK to show or retune the true frequency. **Errors:** none. **See also:** `RttyDecoder::feed`, tests tagged `RttyDecoder::locked_frequency_offset_hz`.

*(Reference: `src/rtty.rs`)*

### `attempt_character` [@ANCHOR: attempt_character]

`attempt_character` is the step that turns an accepted start edge into text. It re-checks that the start bit
reads SPACE and the stop bit MARK, reads the five data bits, and then applies the shift rules. LTRS and FIGS
codes change shift and print nothing. A space always returns to letters (unshift-on-space, the fldigi
convention). A third consecutive non-digit symbol while in figures is treated as a missed LTRS shift and
the decoder falls back to letters from that character on. Characters printed before that point are not retracted.
*(Reference: `src/rtty.rs`)*
