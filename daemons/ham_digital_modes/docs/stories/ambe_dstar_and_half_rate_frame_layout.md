# Story: What is inside a D-STAR or half-rate AMBE+2 voice frame (ham_digital_modes)

A D-STAR voice frame (and, behind the `ambe_plus_2` Cargo feature, an AMBE+2 half-rate frame as used by DMR, System Fusion and
P25 Phase 2) is 72 bits per 20 ms. Both generations share one outer layer: a `C0` Golay codeword, a second Golay codeword `C1`
that is whitened with a sequence seeded from `C0`'s corrected data, and 11 plus 14 unprotected bits (`C2`, `C3`). Correcting `C0`
and `C1` leaves **49 data bits**, numbered `d[0]` (most significant) to `d[48]`. What differs is how those 49 bits are split into
the nine quantizer indices `b0..b8` and which tables turn the indices into speech parameters. The AMBE+2 half-rate code is off by
default and is built and tested with `cargo test --features ambe_plus_2`.

The functions below are the two directions of that split and the step from indices to a speech model. The outer frame layer
(`parse_frame` / `build_frame`) is separate.

## D-STAR

- Receiving, step one: `dstar_extract_raw_parameters` cuts the 49 data bits into `b0..b8`. `b0` (7 bits) is the pitch index,
  taken from `d[0..6)` plus `d[48]`; `b1` (4 bits, `d[38..42)`) picks the voiced/unvoiced pattern; `b2` (6 bits, `d[6..10)` plus
  `d[42..44)`) is the gain change; `b3` and `b4` (9 and 7 bits) are the two gain-vector codebook indices; `b5..b8` are the
  higher-order coefficient indices for the four spectral blocks. `d[24]` is read by nothing, and `b8` is only filled from three
  bits, so its low bit is always 0. The function is total: any 49-bit value gives a result.
  *(Reference: `src/ambe/float/dstar/decode.rs` -> `[@ANCHOR: dstar_extract_raw_parameters]`)*
- Receiving, step two: `dstar_dequantize` takes the 49 data bits and the decoder's memory of the previous frame and returns either
  speech parameters or a tone. If `b0` is 126 or 127 (the only values D-STAR treats specially) the frame is a tone and the tone
  payload (index and volume, read through a different bit layout) is returned without touching the memory. Otherwise it looks up
  the harmonic count `L` for `b0`, computes the fundamental `f0 = 2^(-4.258618 - 0.021766 b0)` cycles per sample (a map fitted to
  the real chip's output), picks each harmonic's voiced/unvoiced flag from the `b1` pattern, forms the frame gain as
  `2 * DG[b2]` with no memory of the previous gain (chip-fitted; mbelib uses 1 and 0.5), rebuilds the log spectral shape from the
  two gain-vector codebook entries and the higher-order tables with an inverse cosine transform per block, adds a prediction from
  the previous frame's log amplitudes (weight 0.8, resampled when `L` changes), and finally turns log amplitudes into linear
  `Ml`, scaling unvoiced harmonics by `0.2046 / sqrt(w0)`. The decoder memory (`L`, log amplitudes, gain) is replaced by this
  frame's values.
  *(Reference: `src/ambe/float/dstar/decode.rs` -> `[@ANCHOR: dstar_dequantize]`)*
- Transmitting: `dstar_pack_raw_parameters` is the exact inverse of the extractor, writing each index back into its bits of a
  49-bit `d` (and leaving `d[24]` at 0). The result goes to `build_frame` for Golay encoding and whitening.
  *(Reference: `src/ambe/float/dstar/encode.rs` -> `[@ANCHOR: dstar_pack_raw_parameters]`)*
- Transmitting a tone (for example a DTMF digit): `dstar_pack_tone_parameters` builds the 49 bits of a tone frame, the exact
  inverse of the tone decoder. It sets `b0 = 126`, scatters the 8-bit tone index and 8-bit volume over their bits, and encodes the
  top three bits of the index as a three-bit selector (`d[6..9)`) through the three lookup tables the decoder uses. Single tones
  are `index * 31.25` Hz for index 5 to 122; a DTMF digit is `128 + row + 4 * column`.
  *(Reference: `src/ambe/float/dstar/encode.rs` -> `[@ANCHOR: dstar_pack_tone_parameters]`)*

## AMBE+2 half-rate (feature `ambe_plus_2`)

The frame layer is identical; the 49 bits are used differently, every one of them is read, and the pitch index has 7 bits with
values 0 to 119 being speech and 120 to 127 reserved for tone, erasure and silence frames.

- `ambe_plus_2_extract_raw_parameters` splits the 49 bits into `b0` (7 bits: `d[0..4)` and `d[37..40)`), `b1` (5), `b2` (5), `b3`
  (9), `b4` (7), `b5` (5), `b6` (4), `b7` (4) and `b8` (3), following the bit table traced from mbelib's AMBE+2 decoder.
  *(Reference: `src/ambe/float/ambe_plus_2/decode.rs` -> `[@ANCHOR: ambe_plus_2_extract_raw_parameters]`)*
- `ambe_plus_2_dequantize` classifies `b0` first. 120 and 122 (a detected tone or DTMF digit, and a call-progress tone), and 126 and
  127, come back as a tone frame carrying the raw indices (the digit is recovered separately from the tone nibbles of `d`); 121
  and 123 are erasures; 124 and 125 are silence, which resets the decoder memory to `L = 14` with zero gain and amplitudes.
  None of those touch the memory except silence. A speech frame reads `L` and `w0` from the pitch tables, the voicing pattern
  from `b1`, the frame gain as `DG[b2] + 0.5 * previous gain`, and then reconstructs the amplitudes the same way as D-STAR but with
  its own codebooks and a prediction weight of 0.65.
  *(Reference: `src/ambe/float/ambe_plus_2/decode.rs` -> `[@ANCHOR: ambe_plus_2_dequantize]`)*
- `ambe_plus_2_pack_raw_parameters` is the exact inverse of the extractor: it writes each index back into its bits of the 49-bit `d`.
  *(Reference: `src/ambe/float/ambe_plus_2/encode.rs` -> `[@ANCHOR: ambe_plus_2_pack_raw_parameters]`)*
- `ambe_plus_2_pack_tone_parameters` builds a tone frame's 49 bits: `b0` is 120 (or 122 for call progress), the 8-bit tone index is
  written as its low nibble four times and its high nibble twice (the decoder takes a majority vote over the four low-nibble copies, so one damaged copy is outvoted), and the
  12-bit amplitude fills `d[4..16)`.
  *(Reference: `src/ambe/float/ambe_plus_2/encode.rs` -> `[@ANCHOR: ambe_plus_2_pack_tone_parameters]`)*
