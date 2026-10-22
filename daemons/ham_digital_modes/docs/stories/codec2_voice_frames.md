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

- 3200 bps carries 20 ms (160 samples) per frame: voiced flag for each 10 ms half (1 + 1 bits), the pitch
  index `Wo` (7 bits), the frame energy index (5 bits) and ten LSP delta indexes (5 bits each).
  `pack_frame` writes those fields into the 8 bytes and `unpack_frame` reads them back; the pitch and
  energy widths are parameters (the codec passes 7 and 5), the ten LSP fields are always 5 bits.
  *(Reference: `src/codec2_3200/bits.rs` -> `[@ANCHOR: pack_frame]`, `[@ANCHOR: unpack_frame]`)*
- 1600 bps carries 40 ms (320 samples) per frame, i.e. four 10 ms sub-frames: four voiced flags, two
  `Wo`/energy pairs (one per 20 ms) and a single set of ten scalar LSP indexes of 4,4,4,4,4,4,4,3,3,2 bits
  (36 bits) that describes the end of the 40 ms. The layout is voiced0, voiced1, `Wo` a, energy a,
  voiced2, voiced3, `Wo` b, energy b, then the LSPs. `pack_frame_1600` and `unpack_frame_1600` convert
  between those fields and the 8 bytes; a round trip returns every field unchanged.
  *(Reference: `src/codec2_1600/bits.rs` -> `[@ANCHOR: pack_frame_1600]`, `[@ANCHOR: unpack_frame_1600]`)*

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

- `Decoder::decode` (3200 bps) turns 8 bytes into 160 samples: it dequantises the pitch, energy and LSPs,
  interpolates the first 10 ms half from the previous frame, then for each half converts LSPs to LPC,
  builds the harmonic amplitudes and synthesises the half with sinusoidal overlap-add.
  *(Reference: `src/codec2_3200/mod.rs` -> `[@ANCHOR: Decoder::decode]`)*
- `DecoderFixed::decode` is the same decoder in integer arithmetic; it is checked against the real C
  decoder's output on a captured bitstream (correlation above 0.99 and RMS within 10 percent).
  *(Reference: `src/codec2_3200/mod.rs` -> `[@ANCHOR: DecoderFixed::decode]`)*
- `codec2_1600:Decoder::decode` turns 8 bytes into 320 samples: after the scalar LSPs are put back in
  ascending order and kept at least 50 Hz (first gaps) or 100 Hz apart, the previous and new LSPs are
  blended at weights 0.25, 0.5 and 0.75 for sub-frames 0 to 2 while sub-frame 3 uses the new LSPs
  directly; pitch and energy of sub-frames 0 and 2 are interpolated from their neighbours.
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: codec2_1600:Decoder::decode]`)*
- `codec2_1600:DecoderFixed::decode` is its integer-only counterpart.
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: codec2_1600:DecoderFixed::decode]`)*

## Wideband (16 kHz) output: the spectral bridge

Normal Codec2 audio stops at 4 kHz. The opt-in 16 kHz methods keep the decoded 0 to 4 kHz harmonics exactly
as the 8 kHz decoder computes them and, for voiced sub-frames only, extend the harmonic series upward by
fitting an exponential decay to the highest-frequency decoded harmonics (up to 16 of them) and continuing it (a rising fit is held flat).
The 4 to 8 kHz band is therefore an educated guess, not recovered information. Unvoiced sub-frames get no
extension. Every method here returns genuine 16 kHz audio (twice as many samples), and on the float
decoders `decoder.spectral_bridge.enabled = false` switches the extension off while still producing 16 kHz
output. A decoder instance must serve either the 8 kHz or the 16 kHz method for a stream, never both,
because both advance the same inter-frame state; use two decoders to get both rates.

- `Decoder::decode_16k` (3200 bps) returns 320 samples per frame.
  *(Reference: `src/codec2_3200/mod.rs` -> `[@ANCHOR: Decoder::decode_16k]`)*
- `DecoderFixed::decode_16k_fixed` is the integer-only version (compiled with the `codec2_16k_bridge`
  feature, which the default `std` build enables); its bridge is always on for outside callers.
  *(Reference: `src/codec2_3200/mod.rs` -> `[@ANCHOR: DecoderFixed::decode_16k_fixed]`)*
- `codec2_1600:Decoder::decode_16k` returns 640 samples per 40 ms frame.
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: codec2_1600:Decoder::decode_16k]`)*
- `codec2_1600:DecoderFixed::decode_16k_fixed` is the integer-only version for 1600 bps.
  *(Reference: `src/codec2_1600/mod.rs` -> `[@ANCHOR: codec2_1600:DecoderFixed::decode_16k_fixed]`)*
- `SpectralBridgeStateFixed::synthesize_subframe_sb_fixed` is the stage underneath both integer 16 kHz
  decoders: given one decoded sub-frame model it extrapolates the harmonics, places them at their bins of a
  1024-point spectrum (the same 15.625 Hz per bin as the 8 kHz path), inverse-transforms and overlap-adds
  160 samples (10 ms at 16 kHz). Its phase accumulator advances once per sub-frame exactly like the 8 kHz
  synthesis so extrapolated and decoded harmonics stay in phase.
  *(Reference: `src/codec2_3200/spectral_bridge.rs` -> `[@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_fixed]`)*
