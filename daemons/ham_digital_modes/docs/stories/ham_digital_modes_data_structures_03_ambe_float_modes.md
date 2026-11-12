# Float AMBE data structures (ambe/float: ambe_plus_2, dstar, concealment, mbe_encode, mbe_synthesis, tones)

As a developer changing this code, I want every data structure in `src/ambe/float/ambe_plus_2/decode.rs`, `src/ambe/float/ambe_plus_2/encoder.rs`, `src/ambe/float/ambe_plus_2/synthesis.rs`, `src/ambe/float/concealment.rs`, `src/ambe/float/dstar/decode.rs`, `src/ambe/float/dstar/encoder.rs`, `src/ambe/float/dstar/synthesis.rs`, `src/ambe/float/mbe_encode.rs`, `src/ambe/float/mbe_synthesis.rs`, `src/ambe/float/tone_detect.rs` and `src/ambe/float/tone_synthesis.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `RawParameters` [@ANCHOR: ham_digital_modes:decode_raw_parameters_struct]

**Purpose.** The nine raw parameter indices `b0..b8`, extracted from the 49 decoded data bits per the exact bit mapping documented in `mod.rs`'s own doc comment (traced directly from mbelib's real source, not guessed).

**Definition.** `pub struct RawParameters` (struct) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `b0` | `u32` | Pitch / frame-kind code (also selects the harmonic count `L`). |
| `b1` | `u32` | Voiced/unvoiced vector index. |
| `b2` | `u32` | Gain-delta index. |
| `b3` | `u32` | PRBA 2-4 vector index. |
| `b4` | `u32` | PRBA 5-8 vector index. |
| `b5` | `u32` | Higher-order coefficient block 1 index. |
| `b6` | `u32` | Higher-order coefficient block 2 index. |
| `b7` | `u32` | Higher-order coefficient block 3 index. |
| `b8` | `u32` | Higher-order coefficient block 4 index. |

**Used by.** `dequantize`, `extract_raw_parameters`, `only` (functions whose signature names it).

**Notes.** Extracted from the 49 decoded data bits by `extract_raw_parameters` and consumed by `dequantize`.

## `FrameKind` [@ANCHOR: ham_digital_modes:decode_frame_kind_enum]

**Purpose.** `b0`'s own real special-frame ranges (120-127, beyond Annex A's 120 real pitch codes), per mbelib's real `mbe_decodeAmbe2450Parms` branch on `b0`. See `mod.rs`'s own doc comment.

**Definition.** `pub enum FrameKind` (enum) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Speech` | - | `b0` 0-119: a real, voiced/unvoiced speech frame -- the common case. |
| `DetectedTone` | - | `b0` 120: the spec calls this range "erasure," but the real chip uses this exact value for a genuinely *detected or forced* tone/DTMF digit (confirmed via the independent `ECMODE_OUT`/`TONE_FRAME` ground-truth bit, `AMBE_CHIP_VALIDATION_FINDINGS.md` section 40) -- never a real erasure in any capture taken so far. **A caller that treats this the same as [`FrameKind::Erasure`] will silently drop a real tone/DTMF digit.** The digit or frequency is fully recoverable via [`decode_tone_idx`] and [`classify_tone_idx`]. |
| `Erasure` | - | `b0` 121 or 123: a genuinely lost/erased frame per spec, with no real parameters to decode -- unlike `120`/`122`, no stimulus tried so far (detected or forced) has ever produced either of these two values, so they remain believed-genuine erasure codes rather than a third undiscovered tone sub-kind. |
| `CallProgress` | - | `b0` 122: a second, distinct tone-bearing sub-code from `120`, confirmed live via forced generation of DVSI's own `Call Progress` tones (dial/ring/busy). Same caveat as [`FrameKind::DetectedTone`]: recoverable via [`decode_tone_idx`]/[`classify_tone_idx`], not a real erasure. |
| `Silence` | - | `b0` 124-125: silence, with mbelib's own fixed `L=14`, `w0 = 2*pi/32`, fully unvoiced. |
| `Tone` | - | `b0` 126-127: the spec's own documented `Tone` code -- **never observed from this chip**, for either a `TD_ENABLE`-detected tone or a directly *forced* one (`AMBE-3000R` manual's own `TONE` field, Table 98/103): both land in `b0=120`/`122` instead (see [`FrameKind::DetectedTone`]/[`FrameKind::CallProgress`]), confirmed live by feeding the encoder an explicit `TONE_IDX` and reading back its own channel bits. This variant is therefore believed unreachable on this chip/rate, not merely undecoded -- kept for spec completeness and in case some other stimulus (a different rate index, a different chip revision) does reach it. |

**Used by.** `classify_b0` (functions whose signature names it).

## `CallProgressTone` [@ANCHOR: ham_digital_modes:decode_call_progress_tone_enum]

**Purpose.** DVSI's own `Call Progress` sub-range of `TONE_IDX` (`0xA0`/`0xA1`/`0xA2`/`0xFF`, Table 104).

**Definition.** `pub enum CallProgressTone` (enum) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Dial` | - | Dial tone (`TONE_IDX` 0xA0). |
| `Ring` | - | Ring tone (0xA1). |
| `Busy` | - | Busy tone (0xA2). |
| `Inactive` | - | `0xFF`: inactive/invalid -- documented by the manual but not tested live. |

## `ToneIdentity` [@ANCHOR: ham_digital_modes:decode_tone_identity_enum]

**Purpose.** The real-world meaning of a [`decode_tone_idx`] result, covering every one of `TONE_IDX`'s documented sub-ranges (Table 104) in one place, rather than leaving each caller to re-derive the range boundaries [`dtmf_digit_from_tone_idx`] alone doesn't cover. A `b0` of [`FrameKind:: DetectedTone`] or [`FrameKind::CallProgress`] pairs with this; see `decode_tone_idx`'s own doc comment for which sub-range is chip-validated live and which is documented-but-untested.

**Definition.** `pub enum ToneIdentity` (enum) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Variant | Type | Meaning |
|---|---|---|
| `Dtmf` | - | A DTMF digit, as `(row, column)` per [`dtmf_digit_from_tone_idx`]'s own layout. |
| `SingleTone` | - | A single tone's frequency in Hz, `tone_idx as f64 * 31.25`. |
| `CallProgress` | `(CallProgressTone)` | A call-progress tone (dial, ring or busy, or inactive). |
| `Reserved` | `(u8)` | A `TONE_IDX` value outside every documented sub-range -- not necessarily invalid, just not one this crate has identified a meaning for yet. |

**Used by.** `classify_tone_idx` (functions whose signature names it).

## `DecoderState` [@ANCHOR: ham_digital_modes:ambe_plus_2_decode_decoder_state_struct]

**Purpose.** Persistent decoder state across frames -- the previous frame's own `L`, its (post-inverse-DCT) `log2` spectral-amplitude history, and `gamma`, mirroring mbelib's own `prev_mp` argument (and `ambe_dstar::decode::DStarDecoderState`'s identical role for the sibling generation).

**Definition.** `pub struct DecoderState` (struct) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Harmonic count of the previous frame. |
| `log2_ml` | `Vec<f64>` | The previous frame's log2 spectral amplitudes after the inverse DCT. |
| `gamma` | `f64` | The gain state `gamma` of the previous frame. |

**Used by.** `dequantize` (functions whose signature names it).

**Notes.** Mirrors mbelib's `prev_mp`; one per stream and mutated by every `dequantize`.

## `Parameters` [@ANCHOR: ham_digital_modes:decode_parameters_struct]

**Purpose.** One frame's own real, decoded semantic parameters.

**Definition.** `pub struct Parameters` (struct) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/decode.rs`; derives `Debug`, `Clone`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Number of harmonics. |
| `w0` | `f64` | Fundamental frequency, radians per sample. |
| `voiced` | `Vec<bool>` | Voiced flag per harmonic (1-indexed). |
| `ml` | `Vec<f64>` | Reconstructed spectral amplitudes (1-indexed). |

## `DequantizedFrame` [@ANCHOR: ham_digital_modes:ambe_plus_2_decode_dequantized_frame_enum]

**Purpose.** The real outcome of dequantizing one frame: a genuine speech frame's parameters, or one of the three special frame kinds `mod.rs`'s doc comment describes -- never a panic on any of the 128 possible `b0` values.

**Definition.** `pub enum DequantizedFrame` (enum) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/decode.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Speech` | `(Parameters)` | A normal speech frame with its dequantized `Parameters`. |
| `Erasure` | - | `b0=121` or `123` only -- a genuine erasure with no real parameters. **Not** `b0=120`/`122`; those carry real tone/DTMF/Call-Progress content and are reported as [`DequantizedFrame:: Tone`] instead, via [`decode_tone_idx`]/[`classify_tone_idx`] on the frame's own `d`. |
| `Silence` | - | mbelib's own fixed silence-frame parameters (`L=14`, `w0 = 2*pi/32`, fully unvoiced) -- carried here rather than discarded, since a real caller synthesizing audio still needs them. |
| `Tone` | - | `b0` in `{120, 122, 126, 127}` -- any tone-bearing frame kind (see [`FrameKind::DetectedTone`], [`FrameKind::CallProgress`], [`FrameKind::Tone`]). The caller decodes the actual tone/digit via `decode_tone_idx(d)` on the same frame's `d` (not carried in `raw` itself, since `TONE_IDX` lives outside the `b0..b8` scatter). |

**Used by.** `dequantize` (functions whose signature names it).

**Notes.** Result of `dequantize`; never panics for any of the 128 possible `b0` values.

## `Encoder` [@ANCHOR: ham_digital_modes:ambe_plus_2_encoder_encoder_struct]

**Purpose.** Streaming float PCM-to-frame encoder for AMBE+2 half-rate: push 16-bit PCM samples, pull one 72-bit logical frame (held in a `u128`) per 20 ms.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `analyzer` | `FrameAnalyzer` | The `FrameAnalyzer` that buffers samples and produces pitch and spectral analysis of each frame. |
| `high_pass` | `HighPassFilter` | The input `HighPassFilter`. |
| `mirror` | `DecoderState` | The encoder's own copy of the decoder state (`DecoderState`), advanced with every frame it emits, so quantization predicts from what a decoder will hold. |
| `analysis` | `AnalysisState` | The voicing/amplitude analysis state carried between frames (`AnalysisState`). |

**Notes.** One per stream; the methods take `&mut self`.

## `AmbePlus2SynthesisDecoder` [@ANCHOR: ham_digital_modes:ambe_plus_2_synthesis_ambe_plus2_synthesis_decoder_struct]

**Purpose.** Streaming float AMBE+2 half-rate frame-to-PCM decoder: `decode_frame` takes one logical frame and returns 20 ms of PCM.

**Definition.** `pub struct AmbePlus2SynthesisDecoder` (struct) in `daemons/ham_digital_modes/src/ambe/float/ambe_plus_2/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `dequant` | `DecoderState` | Decoder state carried across frames (`DecoderState`). |
| `synth` | `MbeSynthesizer` | The shared MBE synthesizer (`MbeSynthesizer`). |
| `tone` | `ToneSynthesizer` | The tone generator for tone frames (`ToneSynthesizer`). |
| `repeats` | `u32` | Consecutive frames repeated because of channel errors (mbelib's `repeat`). |
| `error_policy` | `ErrorPolicy` | How damaged frames are treated (`ErrorPolicy`). |
| `concealer` | `Concealer` | The error concealer (`Concealer`). |

**Notes.** One decoder per stream; the methods take `&mut self`.

## `Descriptor` [@ANCHOR: ham_digital_modes:float_concealment_descriptor_struct]

**Purpose.** What the continuity prior looks at, computed from one decoded frame.

**Definition.** `pub struct Descriptor` (struct) in `daemons/ham_digital_modes/src/ambe/float/concealment.rs`; derives `Clone`, `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `w0` | `f64` | Fundamental frequency of the frame, radians per sample. |
| `level_db` | `f64` | Mean level of the harmonic amplitudes, dB. |
| `spectrum` | `[f64; 16]` | Log amplitude (dB) at 16 points evenly spaced in relative frequency. |
| `voicing` | `[bool; 8]` | Voicing of 8 evenly spaced frequency bands. |

**Used by.** `continuity_cost`, `decide`, `flat` (functions whose signature names it).

## `ConcealParams` [@ANCHOR: ham_digital_modes:float_concealment_conceal_params_struct]

**Purpose.** Tunable constants; `Default` is the tuned set.

**Definition.** `pub struct ConcealParams` (struct) in `daemons/ham_digital_modes/src/ambe/float/concealment.rs`; derives `Clone`, `Copy`, `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `level_scale_db` | `f64` | Laplace scale of the level change between frames, dB. |
| `shape_scale_db` | `f64` | Laplace scale of the spectral shape change (root mean square over 16 points), dB. |
| `voicing_cost` | `f64` | Cost per voicing band that differs from the previous frame, nats. |
| `pitch_scale` | `f64` | Laplace scale of `|ln(w0 / w0_previous)|` (both frames voiced). |
| `garbage_cost` | `f64` | Cost, in nats, of "this frame is garbage" (how unlikely the received parameters would be if they were random), before adding the cost of an error having happened at all. Repeating the previous frame costs this plus `-ln(P(a raw bit is wrong))`, so on a clean channel a large jump (a real speech onset) is accepted, and on a bad channel it is not. |
| `error_suspicion` | `f64` | Extra cost per corrected error the channel code reported on the received frame, nats (suspicion of a miscorrection). |
| `ber_alpha` | `f64` | Smoothing of the bit error rate estimate and its floor. |
| `ber_floor` | `f64` | Floor of the bit error rate estimate. |
| `flip_gate` | `f64` | Bit corrections are only considered once the estimated error rate exceeds `ber_floor * flip_gate`, so a link that has shown no errors is decoded exactly as received. |
| `fade_per_frame` | `f64` | Amplitude factor applied per consecutive repeated frame. |
| `reset_after` | `u32` | Repeated frames in a row after which the decoder state is reset. |
| `widen_per_repeat` | `f64` | How much the continuity priors widen per consecutive repeated frame (the previous accepted frame gets older). |

**Used by.** `new` (functions whose signature names it).

## `Decision` [@ANCHOR: ham_digital_modes:float_concealment_decision_enum]

**Purpose.** What to do with the frame.

**Definition.** `pub enum Decision` (enum) in `daemons/ham_digital_modes/src/ambe/float/concealment.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Variant | Type | Meaning |
|---|---|---|
| `Accept` | `(usize)` | Use candidate `index`. |
| `Repeat` | `{ scale: f64, reset: bool }` | Repeat the previous frame, scaled by this factor (0 = silence). `reset` says the decoder state should be reset first. |

**Used by.** `decide` (functions whose signature names it).

## `Concealer` [@ANCHOR: ham_digital_modes:float_concealment_concealer_struct]

**Purpose.** The error concealer: keeps the last accepted frame's `Descriptor`, a running bit error rate estimate and the repeat count, and decides for each received frame whether to accept a candidate (possibly with bits flipped) or repeat the previous frame.

**Definition.** `pub struct Concealer` (struct) in `daemons/ham_digital_modes/src/ambe/float/concealment.rs`.

| Field | Type | Meaning |
|---|---|---|
| `params` | `ConcealParams` | The tuned constants (public; `Default` is the tuned set). |
| `previous` | `Option<Descriptor>` | Descriptor of the last accepted frame; `None` at the start and after a reset. |
| `ber` | `f64` | Running estimate of the channel bit error rate (starts at `ber_floor`). |
| `repeats` | `u32` | Consecutive frames repeated since the last accepted one. |

**Notes.** One per stream; owned by the synthesis decoder; `error_rate()` reads the estimate.

## `ParsedFrame` [@ANCHOR: ham_digital_modes:decode_parsed_frame_struct]

**Purpose.** The real FEC/whitening outcome of parsing one raw 72-bit frame: the 49 decoded data bits plus both Golay blocks' own corrected-error counts. **The same caveat the P25 chip-validation investigation found applies here too, since this reuses the identical `[23,12,7]` Golay code**: it is a genuine perfect code (covering radius equals packing radius, exactly 3), so *every* possible 23-bit input decodes to *some* codeword within distance <=3 -- `epsilon_c0`/`epsilon_c1` can never exceed 3 regardless of whether the input was ever a real, intentionally-encoded codeword. A single frame's own low corrected-error count is therefore not, by itself, strong evidence of correct framing (see `AMBE_CHIP_VALIDATION_FINDINGS.md`'s own account of exactly this trap) -- real validation needs either a genuine round trip against known data (this module's own tests do that) or the same kind of multi-frame, perturbation-based empirical check the P25 investigation used against the real chip.

**Definition.** `pub struct ParsedFrame` (struct) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `d` | `u64` | The 49 decoded data bits, packed MSB-first into a `u64` (bit 48 down to bit 0). |
| `epsilon_c0` | `u32` | Errors corrected by the first Golay block. |
| `epsilon_c1` | `u32` | Errors corrected by the second Golay block. |

**Used by.** `parse_frame` (functions whose signature names it).

**Notes.** Each count is at most 3 whatever the input, because the Golay code is perfect; a low count alone is therefore not proof of correct framing.

## `RawParameters` [@ANCHOR: ham_digital_modes:dstar_decode_raw_parameters_struct]

**Purpose.** The nine raw parameter indices `b0..b8`, extracted from the 49 decoded data bits per the exact bit mapping documented in `mod.rs`'s own doc comment (traced directly from mbelib's real source, not guessed).

**Definition.** `pub struct RawParameters` (struct) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `b0` | `u32` | Pitch / frame-kind code (also selects the harmonic count `L`). |
| `b1` | `u32` | Voiced/unvoiced vector index. |
| `b2` | `u32` | Gain-delta index. |
| `b3` | `u32` | PRBA 2-4 vector index. |
| `b4` | `u32` | PRBA 5-8 vector index. |
| `b5` | `u32` | Higher-order coefficient block 1 index. |
| `b6` | `u32` | Higher-order coefficient block 2 index. |
| `b7` | `u32` | Higher-order coefficient block 3 index. |
| `b8` | `u32` | Higher-order coefficient block 4 index. |

**Used by.** `extract_raw_parameters`, `only` (functions whose signature names it).

**Notes.** Extracted from the 49 decoded data bits by `extract_raw_parameters` and consumed by `dequantize`; for D-STAR frames of kind `Tone`, `b1` and `b2` are not speech fields (see `decode_tone`).

## `FrameKind` [@ANCHOR: ham_digital_modes:dstar_decode_frame_kind_enum]

**Purpose.** D-STAR's own real special-value trigger for `b0`, traced directly from mbelib's real `ambe3600x2400.c` (`if ((b0&0x7E) == 0x7E) // frame is tone`) -- confirmed against a real, captured chip frame under `ECMODE_IN`'s `TD_ENABLE` bit (see `AMBE_CHIP_VALIDATION_FINDINGS.md`'s cross-mode DTX/DTMF section), not merely read off the source. This is narrower than AMBE+2 half-rate's own 120-127 eight-value block ([`crate::ambe::float::ambe_plus_2::decode::classify_b0`]): D-STAR only ever special-cases `b0` being *exactly* 126 or 127, treating every other value -- including 120-125 -- as an ordinary (if perhaps unusual) voiced/unvoiced speech pitch code. This matters concretely: the same live chip, configured for D-STAR's RATEP and with `DTX_ENABLE` on, was observed driving silence frames to `b0=120` -- a value this real reference decoder does **not** recognize as special, so a DTX- silence frame under D-STAR is decoded as ordinary (if pitch-unusual) speech by mbelib itself, not just by this crate. That is a real property of D-STAR's own encoding, not a gap this function needs to paper over.

**Definition.** `pub enum FrameKind` (enum) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Speech` | - | `b0` anything except 126/127: a real, voiced/unvoiced speech frame -- the common case, and (per the doc comment above) also what a DTX-silence frame at `b0=120` decodes as. |
| `Tone` | - | `b0` 126 or 127 exactly (`b0 & 0x7E == 0x7E`): a tone frame. Its `b1`/`b2` are **not** the ordinary speech fields this module's own [`extract_raw_parameters`] returns -- see [`decode_tone`] for the real, separately bit-scattered tone index/volume. |

**Used by.** `classify_b0` (functions whose signature names it).

## `ToneKind` [@ANCHOR: ham_digital_modes:dstar_decode_tone_kind_enum]

**Purpose.** A tone frame's own real `index` value, classified per mbelib's real range table:  | `index` range | Meaning | |---|---| | `0..5` | Invalid/reserved -- a real encoder never emits this | | `5..=122` | A single tone at `index * 31.25` Hz | | `123..128` | Invalid/reserved | | `128..=163` | A dual tone (DTMF is one, not the only, dual-tone signal) -- see [`dtmf_digit_from_tone_index`] | | `164..=255` (`index`'s remaining 8-bit values) | Invalid/reserved |

**Definition.** `pub enum ToneKind` (enum) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Variant | Type | Meaning |
|---|---|---|
| `Invalid` | - | Index outside every valid range (0..5, 123..128 and 164..255); a real encoder never emits it. |
| `Single` | `{ hz: f64 }` | A single tone at `index * 31.25` Hz (index 5 to 122). |
| `Dual` | - | A dual tone, index 128 to 163 (DTMF is one such signal). |

**Used by.** `classify_tone_index` (functions whose signature names it).

## `TonePayload` [@ANCHOR: ham_digital_modes:decode_tone_payload_struct]

**Purpose.** A tone frame's own real payload: `index` (see [`classify_tone_index`]/[`ToneKind`] for what it means) and `volume`, an 8-bit level with no further documented structure.

**Definition.** `pub struct TonePayload` (struct) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `index` | `u32` | The tone index, classified by `classify_tone_index`. |
| `volume` | `u32` | An 8-bit level with no further documented structure. |

**Used by.** `decode_tone` (functions whose signature names it).

## `DStarDecoderState` [@ANCHOR: ham_digital_modes:decode_dstar_decoder_state_struct]

**Purpose.** Persistent decoder state across frames -- the previous frame's own `L~`, its (post-inverse-DCT) `log2` spectral-amplitude history, and `γ`, all of which the gain/spectral-magnitude recursion below genuinely needs (mirroring mbelib's own `prev_mp` argument).

**Definition.** `pub struct DStarDecoderState` (struct) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Harmonic count of the previous frame. |
| `log2_ml` | `Vec<f64>` | The previous frame's log2 spectral amplitudes after the inverse DCT. |
| `gamma` | `f64` | The gain state `gamma` of the previous frame. |

**Used by.** `dequantize` (functions whose signature names it).

**Notes.** Mirrors mbelib's `prev_mp`; one per stream and mutated by every `dequantize`.

## `DStarParameters` [@ANCHOR: ham_digital_modes:decode_dstar_parameters_struct]

**Purpose.** One frame's own real, decoded semantic parameters -- harmonic count, fundamental frequency, per-harmonic voicing, and reconstructed spectral amplitudes `Ml[1..=l]` (1-indexed to match the spec-style harmonic numbering `mod.rs`'s own doc comment and this whole module use throughout; index 0 is unused padding).

**Definition.** `pub struct DStarParameters` (struct) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Number of harmonics. |
| `w0` | `f64` | Fundamental frequency, radians per sample. |
| `voiced` | `Vec<bool>` | Voiced flag per harmonic (1-indexed, index 0 unused). |
| `ml` | `Vec<f64>` | Reconstructed spectral amplitudes `Ml[1..=l]` (1-indexed). |

## `DequantizedFrame` [@ANCHOR: ham_digital_modes:dstar_decode_dequantized_frame_enum_alt]

**Purpose.** The real outcome of dequantizing one frame: ordinary speech parameters, or a tone frame's own separately-scattered payload (see [`decode_tone`]) -- never speech parameters computed from a tone frame's ordinary-speech-shaped `b0..b8` misinterpretation, which is exactly what this crate did before `classify_b0`/[`FrameKind::Tone`] existed (see this module's own doc comment history: a live chip capture with `TD_ENABLE` on and a DTMF/tone stimulus reliably produces `b0 in {126,127}`, which the pre-fix `dequantize` ran straight through the voiced-speech path).

**Definition.** `pub enum DequantizedFrame` (enum) in `daemons/ham_digital_modes/src/ambe/float/dstar/decode.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Speech` | `(DStarParameters)` | A normal speech frame with its `DStarParameters`. |
| `Tone` | `(TonePayload)` | A tone frame with its separately scattered `TonePayload`. |

**Used by.** `dequantize` (functions whose signature names it).

## `Encoder` [@ANCHOR: ham_digital_modes:dstar_encoder_encoder_struct_alt]

**Purpose.** Streaming float PCM-to-frame encoder for D-STAR: push 16-bit PCM samples, pull one 72-bit logical frame (held in a `u128`) per 20 ms.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/ambe/float/dstar/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `analyzer` | `FrameAnalyzer` | The `FrameAnalyzer` that buffers samples and produces pitch and spectral analysis of each frame. |
| `high_pass` | `HighPassFilter` | The input `HighPassFilter`. |
| `mirror` | `DStarDecoderState` | The encoder's own copy of the decoder state (`DStarDecoderState`), advanced with every frame it emits. |
| `analysis` | `AnalysisState` | The voicing/amplitude analysis state carried between frames (`AnalysisState`). |

**Notes.** One per stream; the methods take `&mut self`.

## `DStarSynthesisDecoder` [@ANCHOR: ham_digital_modes:dstar_synthesis_dstar_synthesis_decoder_struct]

**Purpose.** Streaming float D-STAR frame-to-PCM decoder: `decode_frame` takes one logical frame and returns 20 ms of PCM.

**Definition.** `pub struct DStarSynthesisDecoder` (struct) in `daemons/ham_digital_modes/src/ambe/float/dstar/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `dequant` | `DStarDecoderState` | Decoder state carried across frames (`DStarDecoderState`). |
| `synth` | `MbeSynthesizer` | The shared MBE synthesizer (`MbeSynthesizer`). |
| `tone` | `ToneSynthesizer` | The tone generator for tone frames (`ToneSynthesizer`). |
| `repeats` | `u32` | Consecutive frames repeated because of channel errors (mbelib's `repeat`). |
| `error_policy` | `ErrorPolicy` | How damaged frames are treated (`ErrorPolicy`). |
| `concealer` | `Concealer` | The error concealer (`Concealer`). |

**Notes.** One decoder per stream; the methods take `&mut self`.

## `ModeTables` [@ANCHOR: ham_digital_modes:float_mbe_encode_mode_tables_struct]

**Purpose.** The per-mode tables the quantizer searches.

**Definition.** `pub struct ModeTables<'a>` (struct) in `daemons/ham_digital_modes/src/ambe/float/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `vuv` | `&'a [[bool; 8]]` | Voiced/unvoiced vector table (8 band flags per entry). |
| `dg` | `&'a [f64]` | Gain-delta table. |
| `prba24` | `&'a [[f64; 3]]` | PRBA 2-4 vector table (3 values per entry). |
| `prba58` | `&'a [[f64; 4]]` | PRBA 5-8 vector table (4 values per entry). |
| `lmprbl` | `&'a [[u32; 4]]` | Per-harmonic-count DCT block lengths (4 per entry). |
| `hoc` | `[&'a [[f64; 4]]; 4]` | The four higher-order-coefficient tables (`b5` to `b8`), 4 values per entry. |
| `hoc_b8_even_only` | `bool` | D-STAR/AMBE+2 `b8` only ever carries even indices (its low bit is always 0). |
| `rho` | `f64` | The mode's amplitude-predictor weight (`0.65` for AMBE+2, `dstar::decode::PREDICTOR_RHO` for D-STAR). |
| `gamma_scale` | `f64` | The mode's gain recursion `gamma = gamma_scale*DG + gamma_memory*gamma_prev` (see `dstar::decode::GAMMA_SCALE`). |
| `gamma_memory` | `f64` | The gain recursion's memory factor. |

**Used by.** `quantize_speech` (functions whose signature names it).

**Notes.** Built per mode by `mode_tables`; borrowed (`'a`) from the mode's static tables.

## `SpeechTarget` [@ANCHOR: ham_digital_modes:float_mbe_encode_speech_target_struct]

**Purpose.** What the frame's analysis wants the decoder to reproduce. `voiced` and `ml` are 1-indexed by harmonic (index 0 unused), length `l + 1`.

**Definition.** `pub struct SpeechTarget<'a>` (struct) in `daemons/ham_digital_modes/src/ambe/float/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Number of harmonics. |
| `w0` | `f64` | Fundamental frequency, radians per sample. |
| `vuv_f0` | `f64` | The fundamental used for the V/UV slot lookup `jl = floor(l * 16 * f0)`, in cycles/sample. |
| `voiced` | `&'a [bool]` | Voiced flag per harmonic, 1-indexed (length `l + 1`). |
| `ml` | `&'a [f64]` | Target harmonic amplitudes, 1-indexed. |

**Used by.** `quantize_speech` (functions whose signature names it).

## `PrevState` [@ANCHOR: ham_digital_modes:float_mbe_encode_prev_state_struct]

**Purpose.** The decoder-side state the recursion predicts from (`DStarDecoderState` / `DecoderState` fields).

**Definition.** `pub struct PrevState<'a>` (struct) in `daemons/ham_digital_modes/src/ambe/float/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Harmonic count of the previous frame. |
| `log2_ml` | `&'a [f64]` | Previous frame's log2 amplitudes, 1-indexed per harmonic. |
| `gamma` | `f64` | Previous frame's gain state. |

**Used by.** `prev_at`, `quantize_speech` (functions whose signature names it).

**Notes.** A borrowed view of the encoder's mirror decoder state.

## `QuantizedSpeech` [@ANCHOR: ham_digital_modes:float_mbe_encode_quantized_speech_struct]

**Purpose.** The eight quantizer indices `b1` to `b8` the encoder chose for one speech frame: V/UV pattern, gain, PRBA 2-4, PRBA 5-8 and the four HOC blocks. Plain value type.

**Definition.** `pub struct QuantizedSpeech` (struct) in `daemons/ham_digital_modes/src/ambe/float/mbe_encode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `b1` | `u32` | Voiced/unvoiced vector index. |
| `b2` | `u32` | Gain-delta index. |
| `b3` | `u32` | PRBA 2-4 vector index. |
| `b4` | `u32` | PRBA 5-8 vector index. |
| `b5` | `u32` | Higher-order coefficient block 1 index. |
| `b6` | `u32` | Higher-order coefficient block 2 index. |
| `b7` | `u32` | Higher-order coefficient block 3 index. |
| `b8` | `u32` | Higher-order coefficient block 4 index. |

**Used by.** `quantize_speech` (functions whose signature names it).

## `AnalysisState` [@ANCHOR: ham_digital_modes:float_mbe_encode_analysis_state_struct]

**Purpose.** Frame-to-frame state of the voicing/amplitude analysis (energy tracker and previous band decisions).

**Definition.** `pub struct AnalysisState` (struct) in `daemons/ham_digital_modes/src/ambe/float/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `xi_max` | `f64` | The running maximum energy tracker of the voicing/amplitude analysis. |
| `prev_bands` | `Vec<bool>` | The previous frame's voiced/unvoiced decision per frequency band. |

**Used by.** `analyze_at_pitch` (functions whose signature names it).

**Notes.** Updated by every analysed frame; one per stream.

## `ErrorPolicy` [@ANCHOR: ham_digital_modes:mbe_synthesis_error_policy_enum]

**Purpose.** How a decoder treats frames the channel decoder flags as damaged (`epsilon_c0`/`epsilon_c1` are the numbers of bit errors the two Golay blocks corrected).  [`ErrorPolicy::Concealing`] (the default) is the audio-quality policy of [`crate::ambe::float::concealment`]; on a stream with no channel errors it decodes bit-for-bit like [`ErrorPolicy::Clean`]. [`ErrorPolicy::Clean`] is mbelib's: a speech frame with more than 3 corrected errors in total repeats the previous frame's parameters without touching the predictor, and after 3 repeats in a row the decoder mutes and restarts. [`ErrorPolicy::ChipCompatible`] reproduces what the real chip does, measured with `examples/{dstar,ambe_plus_2}_field_scan.rs ... errclass`: it repeats a frame whenever the first Golay block corrected 3 errors (whatever the second block did), and after 3 repeats in a row it decodes every further damaged frame as received instead of muting, which produces loud bursts and chirps. That is a defect, so it exists only to let conformance tests compare against the chip without the difference hiding others; nothing should select it in normal operation.

**Definition.** `pub enum ErrorPolicy` (enum) in `daemons/ham_digital_modes/src/ambe/float/mbe_synthesis.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Default`.

| Variant | Type | Meaning |
|---|---|---|
| `Clean` | - | mbelib's policy (repeat three times, then mute): kept for reference and for tests; not the default. |
| `ChipCompatible` | - | Reproduce what the real DVSI chip does (repeat whenever the first Golay block corrected 3 errors; after 3 repeats decode damaged frames as received). That produces loud bursts, so it exists only for conformance tests against the chip. |
| `Concealing` | - | Audio-quality concealment ([`crate::ambe::float::concealment`]): graded, continuity-aware repair of the raw bits and a fading repeat instead of a hard mute. Implemented in the D-STAR and AMBE+2 synthesis decoders. |

## `BadFrameAction` [@ANCHOR: ham_digital_modes:mbe_synthesis_bad_frame_action_enum]

**Purpose.** What to do with one damaged frame.

**Definition.** `pub enum BadFrameAction` (enum) in `daemons/ham_digital_modes/src/ambe/float/mbe_synthesis.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Repeat` | - | Repeat the previous frame's parameters. |
| `Mute` | - | Mute the output and restart the decoder. |
| `Decode` | - | Decode the damaged frame as received. |

**Used by.** `bad_frame_action` (functions whose signature names it).

## `MbeSynthesizer` [@ANCHOR: ham_digital_modes:float_mbe_synthesis_mbe_synthesizer_struct]

**Purpose.** The mode-independent float MBE frame synthesizer that D-STAR and AMBE+2 share: it turns one dequantized frame into 20 ms of PCM.

**Definition.** `pub struct MbeSynthesizer` (struct) in `daemons/ham_digital_modes/src/ambe/float/mbe_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `synthesis` | `SynthesisState` | The underlying `SynthesisState`. |
| `error_rate_prev` | `f64` | Previous frame's error-rate estimate, for the smoothing thresholds. |
| `lift_history` | `[f64; 2]` | State of the high-frequency lift filter (the last two samples). |

**Notes.** One per stream; takes `&mut self`.

## `DetectedTone` [@ANCHOR: ham_digital_modes:float_tone_detect_detected_tone_enum]

**Purpose.** What the float tone detector found in a 20 ms block of audio: a DTMF digit or a single tone.

**Definition.** `pub enum DetectedTone` (enum) in `daemons/ham_digital_modes/src/ambe/float/tone_detect.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Variant | Type | Meaning |
|---|---|---|
| `Dtmf` | - | A DTMF digit as the low-group `row` and high-group `col` indices. |
| `Single` | - | `index = round(f / 31.25)`, `hz` the measured frequency. |

## `Detection` [@ANCHOR: ham_digital_modes:float_tone_detect_detection_struct]

**Purpose.** One detection result of the tone detector: which tone and how strong.

**Definition.** `pub struct Detection` (struct) in `daemons/ham_digital_modes/src/ambe/float/tone_detect.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `tone` | `DetectedTone` | The detected tone. |
| `amplitude` | `f64` | Per-tone peak amplitude in PCM units. |

**Used by.** `detect_tone` (functions whose signature names it).

## `ToneSynthesizer` [@ANCHOR: ham_digital_modes:float_tone_synthesis_tone_synthesizer_struct]

**Purpose.** Two phase accumulators (radians); a tone uses one or both.

**Definition.** `pub struct ToneSynthesizer` (struct) in `daemons/ham_digital_modes/src/ambe/float/tone_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `phase` | `[f64; 2]` | Two phase accumulators in radians; a tone uses one or both. |

**Notes.** Advanced by every synthesized frame so tones are continuous across frames.

