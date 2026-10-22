# Story: BPSK31 text to audio and back (ham_digital_modes)

An operator typing into a PSK31 window gets Varicode text turned into a narrow, phase-keyed audio tone, and
gets text back out of received audio. The pipeline is pure Rust, runs at 31.25 baud, and has no forward error
correction (as in the original mode, the operator notices garbles and asks for a repeat).

## Sending

`psk31_encode_bits` turns text into the Varicode bitstream: each ASCII character's codeword (1 to 10 bits,
most significant bit first, always starting and ending in a 1) followed by two zero bits that mark the
character boundary. Bytes at or above 128 are skipped rather than guessed at.
*(Reference: `src/psk31.rs` -> `[@ANCHOR: psk31_encode_bits]`)*

`psk31_modulate` turns those bits into 16-bit mono PCM at the requested carrier frequency and sample rate.
Each symbol lasts `sample_rate / 31.25` samples (rounded). A 0 bit reverses the carrier phase by 180 degrees,
a 1 bit leaves it steady, and every symbol is shaped by a raised-cosine amplitude envelope that reaches zero
at the symbol edges, so the reversals do not splatter outside the roughly 31 Hz channel.
*(Reference: `src/psk31.rs` -> `[@ANCHOR: psk31_modulate]`)*

## Receiving

`psk31_demodulate` recovers the bitstream from a complete buffer of audio at a known carrier frequency. It
assumes symbol timing starts at sample 0 of the buffer and does no carrier search or tracking, so it is for
clean or loopback signals. Each full symbol period is correlated against the carrier, and a symbol is a 1
when its phase is within 90 degrees of the previous symbol's phase and a 0 otherwise. The first symbol is
always read as 1, because every Varicode codeword starts with a 1. A trailing partial symbol is dropped, and a
sample rate too low to give one sample per symbol returns no bits.
*(Reference: `src/psk31.rs` -> `[@ANCHOR: psk31_demodulate]`)*

`psk31_decode_bits` turns the bitstream back into text. A run of two or more zeros ends a character; because no
codeword ends in a 0, the first zero of the run belongs to the gap and is dropped before the codeword is
looked up. Codewords that match nothing in the table are discarded silently. A run of more than 11 bits with
no gap is treated as noise and the accumulator restarts, so demodulated noise cannot overflow it. The limit
is 10 codeword bits plus one provisional gap bit, which is what lets the ten printable characters with
10-bit codewords (for example `Z`, `?`, `@`, `%`) decode correctly.
*(Reference: `src/psk31.rs` -> `[@ANCHOR: psk31_decode_bits]`)*

## Live streaming

`Psk31Decoder::feed_with_iq` is the incremental form used by the live audio pipeline. It takes whatever number
of samples arrived, keeps symbol timing, carrier phase and partial Varicode bits across calls, and returns the
characters completed by this call together with one raw (I, Q) correlation pair for every symbol processed.
Those pairs are exactly the numbers the bit decisions are made from, which is what the browser constellation
display plots. A call usually returns zero or one point, never a guaranteed one. `Psk31Decoder::feed` is the
same call with the I/Q discarded.
*(Reference: `src/psk31.rs` -> `[@ANCHOR: Psk31Decoder::feed_with_iq]`)*
