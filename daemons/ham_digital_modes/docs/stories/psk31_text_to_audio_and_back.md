# Story: BPSK31 text to audio and back (ham_digital_modes)

An operator typing into a PSK31 (phase-shift keying at 31 baud, a keyboard digital mode) window gets Varicode text turned into a narrow, phase-keyed audio tone, and
gets text back out of received audio. The pipeline is pure Rust, runs at 31.25 baud, and has no forward error
correction (as in the original mode, the operator notices garbles and asks for a repeat).

## Sending

### `psk31_encode_bits` [@ANCHOR: psk31_encode_bits]

`psk31_encode_bits` turns text into the Varicode bitstream: each ASCII character's codeword (1 to 10 bits,
most significant bit first, always starting and ending in a 1) followed by two zero bits that mark the
character boundary. Bytes at or above 128 are skipped rather than guessed at.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str`, borrowed | Message to send. Characters with a code point of 128 or more are skipped. |

**Returns:** `Vec<bool>`, one element per bit, `true` = 1, in transmit order; empty for empty text. **Errors:** none; the function cannot fail or panic. **Special concerns:** pure and allocation-only (no I/O, no locks, safe from any thread), and it does not key a transmitter: it only produces bits. **Signature:** `pub fn psk31_encode_bits(text: &str) -> Vec<bool>`. **See also:** `psk31_modulate` (next step), `psk31_decode_bits` (the inverse), and the in-file tests tagged `psk31_encode_bits`.

*(Reference: `src/psk31.rs`)*

### `psk31_modulate` [@ANCHOR: psk31_modulate]

`psk31_modulate` turns those bits into 16-bit mono PCM at the requested carrier frequency and sample rate.
Each symbol lasts `sample_rate / 31.25` samples (rounded). A 0 bit reverses the carrier phase by 180 degrees,
a 1 bit leaves it steady, and every symbol is shaped by a raised-cosine amplitude envelope that reaches zero
at the symbol edges, so the reversals do not splatter outside the roughly 31 Hz channel.

| Argument | Type | Meaning |
|---|---|---|
| `bits` | `&[bool]`, borrowed | Bitstream from `psk31_encode_bits`; `false` reverses the phase. |
| `carrier_hz` | `f64` | Audio carrier frequency in hertz, typically 500 to 2500 for an SSB (single sideband) transmitter's passband. |
| `sample_rate` | `u32` | Output samples per second (for example 8000, 12000 or 48000); symbol length is `sample_rate / 31.25` rounded. |

**Returns:** `Vec<i16>`, mono 16-bit PCM, `bits.len() * samples_per_symbol` samples, full scale of the envelope peak (about +/-32767). **Errors:** none; a sample rate under 16 Hz yields zero samples per symbol and an empty result. **Special concerns:** pure and synchronous; the carrier phase is continuous across symbols except for the intended reversals. The function makes audio only. Whatever plays it into a radio must go through the relay's transmit lock, band check and transmit gate; this crate never keys a transmitter. **Signature:** `pub fn psk31_modulate(bits: &[bool], carrier_hz: f64, sample_rate: u32) -> Vec<i16>`. **See also:** tests tagged `psk31_modulate`, and `psk31_demodulate` as the inverse.

*(Reference: `src/psk31.rs`)*

## Receiving

### `psk31_demodulate` [@ANCHOR: psk31_demodulate]

`psk31_demodulate` recovers the bitstream from a complete buffer of audio at a known carrier frequency. It
assumes symbol timing starts at sample 0 of the buffer and does no carrier search or tracking, so it is for
clean or loopback signals. Each full symbol period is correlated against the carrier, and a symbol is a 1
when its phase is within 90 degrees of the previous symbol's phase and a 0 otherwise. The first symbol is
always read as 1, because every Varicode codeword starts with a 1. A trailing partial symbol is dropped, and a
sample rate too low to give one sample per symbol returns no bits.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]`, borrowed | Complete mono PCM (pulse-code modulation) buffer whose first sample starts a symbol. |
| `carrier_hz` | `f64` | Carrier frequency in hertz; must be close to the real one, since no search is done. |
| `sample_rate` | `u32` | Samples per second of `samples`. |

**Returns:** `Vec<bool>`, one bit per complete symbol; empty when `samples_per_symbol` rounds to zero. **Errors:** none. **Special concerns:** pure; cost is one correlation per sample. For live audio use `Psk31Decoder` instead, which keeps state between chunks. **Signature:** `pub fn psk31_demodulate(samples: &[i16], carrier_hz: f64, sample_rate: u32) -> Vec<bool>`. **See also:** tests tagged `psk31_demodulate`, `correlate_symbol_iq` and `phase_to_bit` (the helpers that compute the I/Q sums and the bit decision).

*(Reference: `src/psk31.rs`)*

### `psk31_decode_bits` [@ANCHOR: psk31_decode_bits]

`psk31_decode_bits` turns the bitstream back into text. A run of two or more zeros ends a character; because no
codeword ends in a 0, the first zero of the run belongs to the gap and is dropped before the codeword is
looked up. Codewords that match nothing in the table are discarded silently. A run of more than 11 bits with
no gap is treated as noise and the accumulator restarts, so demodulated noise cannot overflow it. The limit
is 10 codeword bits plus one provisional gap bit, which is what lets the ten printable characters with
10-bit codewords (for example `Z`, `?`, `@`, `%`) decode correctly.

| Argument | Type | Meaning |
|---|---|---|
| `bits` | `&[bool]`, borrowed | Demodulated bitstream, in order, including the zero gaps between characters. |

**Returns:** `String` of decoded characters; empty when no codeword completed. **Errors:** none; unknown codewords are dropped without a message. **Special concerns:** pure; the accumulator is local to the call, so a character split across two calls is lost (the streaming `Psk31Decoder` keeps it instead). **Signature:** `pub fn psk31_decode_bits(bits: &[bool]) -> String`. **See also:** tests tagged `psk31_decode_bits`, and `VaricodeAccumulator::push_bit`, the shared per-bit logic.

*(Reference: `src/psk31.rs`)*

## Live streaming

### `Psk31Decoder::feed_with_iq` [@ANCHOR: Psk31Decoder::feed_with_iq]

`Psk31Decoder::feed_with_iq` is the incremental form used by the live audio pipeline. It takes whatever number
of samples arrived, keeps symbol timing, carrier phase and partial Varicode bits across calls, and returns the
characters completed by this call together with one raw (I, Q) correlation pair for every symbol processed.
Those pairs are exactly the numbers the bit decisions are made from, which is what the browser constellation
display plots. A call usually returns zero or one point, never a guaranteed one. `Psk31Decoder::feed` is the
same call with the I/Q discarded.
*(Reference: `src/psk31.rs`)*

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `Psk31Decoder` | Decoder state: pending partial symbol, running sample index, previous symbol phase and the Varicode accumulator. Mutated on every call. |
| `samples` | `&[i16]`, borrowed | Newly arrived mono PCM at the rate given to `Psk31Decoder::new`; any length, including zero. |

**Returns:** `(String, Vec<(f64, f64)>)`: the characters completed by this call, and one `(I, Q)` correlation pair per symbol consumed, in order. **Errors:** none. **Special concerns:** not thread-safe by itself (needs `&mut`); the caller owns serialisation. Samples that do not fill a symbol are kept for the next call, so the memory use is bounded by one symbol. It never transmits. **Signature:** `pub fn feed_with_iq(&mut self, samples: &[i16]) -> (String, Vec<(f64, f64)>)`. **See also:** the test tagged `Psk31Decoder::feed_with_iq`, `Psk31Decoder::feed`, and the constellation display described in `DIGITAL_MODES.md`.
