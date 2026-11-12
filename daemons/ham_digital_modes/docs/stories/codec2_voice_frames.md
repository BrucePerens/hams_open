# Story: Codec2 3200 and 1600 Voice Frames, Speech In and Out (ham_digital_modes)

This is what an operator (or a radio application built on this crate) gets from the two Codec2 vocoder
modes: hand over a block of 8 kHz, 16-bit speech and receive a 64-bit frame (8 bytes), or hand over a
frame and receive speech back. There are two modes with the same frame size but different cadence, and
each exists as a floating-point implementation and as an integer-only ("fixed") implementation that
builds without a floating-point unit. The decoders are bit-compatible with the real Codec2 bitstream; the
encoders produce frames that any compliant decoder decodes, but they are free to pick different
quantiser indexes from the reference encoder for the same speech, so only decodability and intelligibility
(not bit equality with a C encoder) can be compared.

## The two frame formats

Both modes put exactly 64 bits in a frame, packed most-significant-bit first, with every field wider
than one bit Gray-coded so that a one-step quantiser error costs a single bit.

- 3200 bps carries 20 ms (160 samples) per frame: voiced flag for each 10 ms half (1 + 1 bits), the pitch index `Wo` (7 bits), the frame energy index (5 bits) and ten LSP delta indexes (5 bits each).
- 1600 bps carries 40 ms (320 samples) per frame, i.e. four 10 ms sub-frames: four voiced flags, two `Wo`/energy pairs (one per 20 ms) and a single set of ten scalar LSP indexes of 4,4,4,4,4,4,4,3,3,2 bits (36 bits) that describes the end of the 40 ms. The layout is voiced0, voiced1, `Wo` a, energy a, voiced2, voiced3, `Wo` b, energy b, then the LSPs.

The four packing functions, each documented below, convert between those fields and the 8 bytes.

### `pack_frame` [@ANCHOR: pack_frame]

Writes the fields of one 3200 bps frame into 8 bytes, most significant bit first, with every field wider than one bit Gray-coded. It is the last step of encoding and is also what the tests use to build frames.

| Argument | Type | Meaning |
|---|---|---|
| `fields` | `&FrameFields`, borrowed | `voiced0`, `voiced1` (bool), `wo_index` and `e_index` (u32 indexes) and `lsp_indexes: [u32; 10]` (each 0 to 31). |
| `wo_bits` | `u32` | Width of the pitch field; the codec passes 7. |
| `e_bits` | `u32` | Width of the energy field; the codec passes 5. |

**Returns:** `[u8; 8]`. **Errors:** none and no panic for in-range indexes; an index too wide for its field trips a `debug_assert` in debug builds, and in release builds its extra bits are ORed into neighbouring fields and corrupt them, so callers must range-limit first. **Special concerns:** pure. **Signature:** `pub fn pack_frame(fields: &FrameFields, wo_bits: u32, e_bits: u32) -> [u8; 8]`. **See also:** `unpack_frame`, test `pack_frame_matches_the_real_reference_bits_on_real_captured_field_values`.

### `unpack_frame` [@ANCHOR: unpack_frame]

The inverse of `pack_frame`: reads one 3200 bps frame's fields back from its 8 bytes, undoing the Gray code. Called by every 3200 bps decoder first.

| Argument | Type | Meaning |
|---|---|---|
| `bytes` | `&[u8; 8]`, borrowed | The frame. Every 64-bit pattern is legal. |
| `wo_bits`, `e_bits` | `u32` | Field widths, as for `pack_frame`. |

**Returns:** `FrameFields`, the voiced flags, the two indexes and the ten 5-bit LSP indexes. **Errors:** none; total on all inputs. **Special concerns:** pure. **Signature:** `pub fn unpack_frame(bytes: &[u8; 8], wo_bits: u32, e_bits: u32) -> FrameFields`. **See also:** `pack_frame` and its test.

### `pack_frame_1600` [@ANCHOR: pack_frame_1600]

Writes one 1600 bps frame (40 ms) into 8 bytes, with the layout given above and Gray coding of the multi-bit fields.

| Argument | Type | Meaning |
|---|---|---|
| `fields` | `&FrameFields1600`, borrowed | `voiced0..voiced3` (bool), `wo_index_a`, `e_index_a`, `wo_index_b`, `e_index_b` (u32) and `lsp_indexes: [u32; 10]` of the widths 4,4,4,4,4,4,4,3,3,2 bits. |

**Returns:** `[u8; 8]`. **Errors:** none. **Special concerns:** pure. **Signature:** `pub fn pack_frame_1600(fields: &FrameFields1600) -> [u8; 8]`. **See also:** `unpack_frame_1600`, test `pack_unpack_round_trips_arbitrary_field_values`.

### `unpack_frame_1600` [@ANCHOR: unpack_frame_1600]

The inverse of `pack_frame_1600`; a round trip returns every field unchanged.

| Argument | Type | Meaning |
|---|---|---|
| `bytes` | `&[u8; 8]`, borrowed | The frame; every pattern is legal. |

**Returns:** `FrameFields1600`. **Errors:** none. **Special concerns:** pure. **Signature:** `pub fn unpack_frame_1600(bytes: &[u8; 8]) -> FrameFields1600`. **See also:** `pack_frame_1600` and the same round-trip test.

Every possible 64-bit pattern is a legal frame (all indexes are in range for their field), so a corrupted
frame still decodes to some audio instead of failing; the synthesis stage's ear protection limits the
level if bit errors would otherwise produce a very loud spike.

## Speech to frame (encoding)

- **3200 bps, floating-point reference.** `Encoder::encode` takes 160 samples (two 10 ms halves). It
  advances the pitch estimator and the voiced/unvoiced decision once per half, takes the second half's
  pitch as the transmitted `Wo`, then runs one linear-prediction analysis over the whole 320-sample history
  window (windowing, autocorrelation, white-noise correction, Levinson-Durbin, bandwidth expansion, LSP
  conversion; evenly spaced fallback LSPs if the root search fails) and quantises the energy and LSP
  deltas. This is the validated reference the integer encoder is compared against, not what the 3200 bps
  product path ships.
  *(Reference: `src/codec2_3200/floating_reference/mod.rs` -> `[@ANCHOR: Encoder::encode]`)*
- **3200 bps, integer-only.** `EncoderFixed::encode` does the same job in `i64` fixed-point arithmetic with
  no floating point at run time; the pitch comes out of the estimator as a bin index that a committed
  table converts to the `Wo` index. Its decoded output is checked against the float encoder's by
  correlation, not bit equality.
  *(Reference: `src/codec2_3200/encoder_fixed.rs` -> `[@ANCHOR: EncoderFixed::encode]`)*
- **1600 bps, floating-point.** `codec2_1600:Encoder::encode` takes 320 samples and runs four 10 ms
  analysis steps; sub-frames 0 and 2 contribute only a voiced flag, sub-frames 1 and 3 add a `Wo` and
  energy, and only the LSPs from the last step are quantised and sent. Unlike 3200 bps, this float encoder
  is the live 1600 bps encoder (1600 reuses the float pitch, voicing and LPC chain).
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: codec2_1600:Encoder::encode]`)*
- **1600 bps, integer-only.** `codec2_1600:EncoderFixed::encode` mirrors it with the integer pitch
  estimator, voicing decision, LPC chain and the integer scalar LSP quantiser.
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: codec2_1600:EncoderFixed::encode]`)*

## Frame to speech (decoding)

Each decoder object remembers the previous frame's pitch, voicing, LSPs and energy, because the sub-frames
that are not transmitted are interpolated between the previous frame's final values and the new ones. A
fresh decoder starts from the lowest pitch, unvoiced, evenly spaced LSPs and unit energy, so the first
frame fades in from a neutral state. Decode frames of one stream in order on one decoder.

### `Decoder::decode` [@ANCHOR: Decoder::decode]

The floating-point 3200 bps decoder: it dequantises the pitch, energy and LSPs, interpolates the first 10 ms half from the previous frame, then for each half converts LSPs to LPC, builds the harmonic amplitudes and synthesises the half with sinusoidal overlap-add.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `Decoder` | Inter-frame memory (previous pitch, voicing, LSPs, energy, synthesis phase); advanced by every call. |
| `bytes` | `&[u8; 8]`, borrowed | One 64-bit frame; every pattern is legal. |

**Returns:** `[i16; 160]`, 20 ms of 8 kHz mono speech. **Errors:** none; corrupted bits give wrong audio, never a failure, and the ear-protection stage limits very loud spikes. **Special concerns:** pure CPU work, no I/O or locks; frames of one stream must be decoded in order on one decoder, and a decoder must serve either this method or `decode_16k`, never both. **Signature:** `pub fn decode(&mut self, bytes: &[u8; 8]) -> [i16; 160]`. **See also:** `DecoderFixed::decode`, `unpack_frame`.

### `DecoderFixed::decode` [@ANCHOR: DecoderFixed::decode]

The same 3200 bps decoder in integer arithmetic, for targets without a floating-point unit. It is checked against the real C decoder's output on a captured bitstream (correlation above 0.99 and RMS within 10 percent), not bit for bit.

**Call interface:** `pub fn decode(&mut self, bytes: &[u8; 8]) -> [i16; 160]`; arguments, return value, errors and the ordering rule are exactly those of `Decoder::decode`. **Special concerns:** no floating-point operation at run time (a test scans the source for float tokens); it uses 64-bit intermediates that compile to library calls on 32-bit cores. **See also:** `Decoder::decode` and the golden-hash and float-versus-fixed tests in `tests/`.

### `codec2_1600:Decoder::decode` [@ANCHOR: codec2_1600:Decoder::decode]

The floating-point 1600 bps decoder. After the scalar LSPs are put back in ascending order and kept at least 50 Hz (first gaps) or 100 Hz apart, the previous and new LSPs are blended at weights 0.25, 0.5 and 0.75 for sub-frames 0 to 2 while sub-frame 3 uses the new LSPs directly; pitch and energy of sub-frames 0 and 2 are interpolated from their neighbours.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `codec2_1600::Decoder` | Inter-frame memory, advanced on every call. |
| `bytes` | `&[u8; 8]`, borrowed | One 40 ms frame; every pattern is legal. |

**Returns:** `[i16; 320]`, 40 ms at 8 kHz. **Errors:** none. **Special concerns:** same as the 3200 bps decoder (in-order frames, one decoder per rate of output, no I/O). **Signature:** `pub fn decode(&mut self, bytes: &[u8; 8]) -> [i16; 320]`. **See also:** `unpack_frame_1600`, `codec2_1600:DecoderFixed::decode`.

### `codec2_1600:DecoderFixed::decode` [@ANCHOR: codec2_1600:DecoderFixed::decode]

The integer-only counterpart of `codec2_1600:Decoder::decode`, for targets without a floating-point unit. **Call interface:** `pub fn decode(&mut self, bytes: &[u8; 8]) -> [i16; 320]`, with the same argument, return value and ordering rules as the float decoder, and no floating-point operation at run time. **Errors:** none. **See also:** `codec2_1600:Decoder::decode`, whose output it approximates (compared by correlation in the tests, not bit equality).

## Wideband (16 kHz) output: the spectral bridge

Normal Codec2 audio stops at 4 kHz. The opt-in 16 kHz methods keep the decoded 0 to 4 kHz harmonics exactly
as the 8 kHz decoder computes them and, for voiced sub-frames only, extend the harmonic series upward by
fitting an exponential decay to the highest-frequency decoded harmonics (up to 16 of them) and continuing it (a rising fit is held flat).
The 4 to 8 kHz band is therefore an educated guess, not recovered information. Unvoiced sub-frames get no
extension. Every method here returns genuine 16 kHz audio (twice as many samples), and on the float
decoders `decoder.spectral_bridge.enabled = false` switches the extension off while still producing 16 kHz
output. A decoder instance must serve either the 8 kHz or the 16 kHz method for a stream, never both,
because both advance the same inter-frame state; use two decoders to get both rates.

### `Decoder::decode_16k` [@ANCHOR: Decoder::decode_16k]

The 3200 bps float decoder with wideband output. The decoded 0 to 4 kHz harmonics are exactly those of `decode`; for voiced sub-frames the harmonic series is extended upward by the spectral bridge.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `Decoder` | Decoder memory; the bridge state is part of it. `decoder.spectral_bridge.enabled = false` turns the extension off while still producing 16 kHz output. |
| `bytes` | `&[u8; 8]`, borrowed | One frame. |

**Returns:** `[i16; 320]`, 20 ms at 16 kHz. **Errors:** none. **Special concerns:** do not mix with `decode` on the same instance (both advance the same inter-frame state); the 4 to 8 kHz band is an educated guess, not recovered information. **Signature:** `pub fn decode_16k(&mut self, bytes: &[u8; 8]) -> [i16; 320]`.

### `DecoderFixed::decode_16k_fixed` [@ANCHOR: DecoderFixed::decode_16k_fixed]

The integer-only wideband decoder for 3200 bps, compiled with the `codec2_16k_bridge` feature (which the default `std` build enables). The bridge is always on for outside callers. **Call interface:** `pub fn decode_16k_fixed(&mut self, bytes: &[u8; 8]) -> [i16; 320]`; same arguments, return value and "one method per decoder instance" rule as `Decoder::decode_16k`. **Errors:** none. **See also:** `SpectralBridgeStateFixed::synthesize_subframe_sb_fixed`, the stage underneath it, and the float-versus-fixed 16 kHz test in `tests/codec2_3200_16k_fixed_vs_float.rs`.

### `codec2_1600:Decoder::decode_16k` [@ANCHOR: codec2_1600:Decoder::decode_16k]

The 1600 bps float decoder with wideband output, using the same bridge as the 3200 bps decoder. **Call interface:** `pub fn decode_16k(&mut self, bytes: &[u8; 8]) -> [i16; 640]`. **Returns:** 640 samples, 40 ms at 16 kHz. **Arguments, errors and concerns:** as `Decoder::decode_16k`; one frame of eight bytes, no failure path, in-order frames, never mixed with the 8 kHz `decode` on one instance. **See also:** `codec2_1600:Decoder::decode`.

### `codec2_1600:DecoderFixed::decode_16k_fixed` [@ANCHOR: codec2_1600:DecoderFixed::decode_16k_fixed]

The integer-only wideband decoder for 1600 bps. **Call interface:** `pub fn decode_16k_fixed(&mut self, bytes: &[u8; 8]) -> [i16; 640]`; returns 640 samples (40 ms at 16 kHz) and has no floating-point arithmetic at run time. Arguments, errors and the one-method-per-instance rule are those of `Decoder::decode_16k`. **See also:** `codec2_1600:Decoder::decode_16k` and `SpectralBridgeStateFixed::synthesize_subframe_sb_fixed`.

### `SpectralBridgeStateFixed::synthesize_subframe_sb_fixed` [@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_fixed]

The stage underneath both integer 16 kHz decoders: given one decoded sub-frame model it extrapolates the harmonics, places them at their bins of a 1024-point spectrum (the same 15.625 Hz per bin as the 8 kHz path), inverse-transforms and overlap-adds 160 samples (10 ms at 16 kHz). Its phase accumulator advances once per sub-frame exactly like the 8 kHz synthesis, so extrapolated and decoded harmonics stay in phase.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `SpectralBridgeStateFixed` | Overlap-add tail and phase memory; mutated every call. |
| `model` | `&ModelFixed`, borrowed | The decoded sub-frame: fundamental, harmonic amplitudes, voicing and `k_q23` (the bin scaling computed once in `ModelFixed::new`, not clamped to half the original FFT size). |

**Returns:** `[i16; 160]`, one 10 ms sub-frame at 16 kHz. **Errors:** none. **Special concerns:** crate-private (`pub(crate)`), integer only, one call per sub-frame in order. **See also:** the test `synthesize_subframe_sb_fixed_produces_finite_reasonably_scaled_audio_enabled_and_disabled`.
