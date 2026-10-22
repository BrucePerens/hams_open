# Story: RTTY text to audio and back (ham_digital_modes)

An operator gets standard amateur radio teletype: 45.45 baud, 170 Hz shift, MARK at the configured audio tone
(2125 Hz by default) and SPACE 170 Hz above it, 5-bit Baudot (ITA2 with the US-TTY figures variant), one
SPACE start bit, five data bits least significant first, and a 1.5-bit MARK stop bit.

## Sending

`char_to_baudot` maps one character, case-insensitively, to its 5-bit code and says whether it needs the
figures shift. Anything with no Baudot code (the table is 5-bit limited by design) yields nothing.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: char_to_baudot]`)*

`text_to_framed_bits` builds the framed bit sequence for a message (true is MARK, false is SPACE). It inserts a
LTRS or FIGS shift character only when the shift state actually changes, starting in letters, and skips
characters that have no code.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: text_to_framed_bits]`)*

`rtty_modulate` turns text into 16-bit mono PCM at 0.9 of full scale as continuous-phase FSK, so there is no
phase jump at MARK/SPACE changes, and stretches every stop bit to 1.5 bit periods.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: rtty_modulate]`)*

## Receiving

RTTY is asynchronous, so a decoder has to find character starts itself. A candidate start is accepted when the
audio before it is sustained MARK, the start bit reads SPACE, the stop bit reads MARK, and the five data bits
show a real two-tone signal; the thresholds are on MARK/SPACE energy ratios, so they work at any signal level.
The search also tries nine carrier offsets from -40 to +40 Hz in 10 Hz steps, so a mistuned receiver still
decodes.

`rtty_demodulate` decodes a whole buffer in one call and has no memory between calls.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: rtty_demodulate]`)*

`RttyDecoder::new` builds the streaming decoder with the full offset search; chunks of any size can be fed and
characters arrive as they complete. It holds back a character until enough audio is buffered to confirm it, so
the last character of a burst can appear one character time (about 150 ms) late, on the next feed.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: RttyDecoder::new]`)*

`RttyDecoder::new_fixed` is the cheap variant for the wide-band channel scanner: one carrier hypothesis, no
offset search. It tolerates about 25 Hz of mistuning cleanly, so a scanner bank that spaces candidates 40 Hz
apart still finds a signal, and the winner is then promoted to a full decoder.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: RttyDecoder::new_fixed]`)*

`RttyDecoder::feed` takes the next chunk of audio and returns the characters completed so far. A sample rate
too low for one sample per bit makes it return nothing without buffering, and already-scanned audio is
dropped once more than ten seconds of it has accumulated.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: RttyDecoder::feed]`)*

`RttyDecoder::locked_frequency_offset_hz` reports the offset of the candidate that decoded the most recent
character, or 0.0 before any character has decoded; for a `new_fixed` decoder it is always 0.0.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: RttyDecoder::locked_frequency_offset_hz]`)*

`attempt_character` is the step that turns an accepted start edge into text. It re-checks that the start bit
reads SPACE and the stop bit MARK, reads the five data bits, and then applies the shift rules. LTRS and FIGS
codes change shift and print nothing. A space always returns to letters (unshift-on-space, the fldigi
convention). A third consecutive non-digit symbol while in figures is treated as a missed LTRS shift and
the decoder falls back to letters from that character on. Characters printed before that point are not retracted.
*(Reference: `src/rtty.rs` -> `[@ANCHOR: attempt_character]`)*
