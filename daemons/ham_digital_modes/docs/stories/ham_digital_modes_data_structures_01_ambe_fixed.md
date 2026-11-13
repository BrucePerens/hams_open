# Fixed-point AMBE data structures (dvsi_p25fec, ambe/fixed: ambe_plus_2, dstar, general)

As a developer changing this code, I want every data structure in `src/ambe/dvsi_p25fec/frame.rs`, `src/ambe/dvsi_p25fec/wire_format.rs`, `src/ambe/fixed/ambe_plus_2/decode.rs`, `src/ambe/fixed/ambe_plus_2/encoder.rs`, `src/ambe/fixed/ambe_plus_2/synthesis.rs`, `src/ambe/fixed/dstar/decode.rs`, `src/ambe/fixed/dstar/encoder.rs`, `src/ambe/fixed/dstar/synthesis.rs`, `src/ambe/fixed/general/concealment.rs`, `src/ambe/fixed/general/mbe_encode.rs`, `src/ambe/fixed/general/mbe_speech.rs`, `src/ambe/fixed/general/mbe_synthesis.rs`, `src/ambe/fixed/general/tone_detect.rs`, `src/ambe/fixed/general/tone_synthesis.rs`, `src/ambe/fixed/general/unvoiced_synthesis.rs` and `src/ambe/fixed/general/voiced_synthesis.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `DecodedBlock` [@ANCHOR: ham_digital_modes:frame_decoded_block_struct]

**Purpose.** One decoded value plus its FEC distance (corrected-bit count for Golay/Hamming blocks, always 0 for the unprotected [`Block::Raw`] `c7` field).

**Definition.** `pub struct DecodedBlock` (struct) in `daemons/ham_digital_modes/src/ambe/dvsi_p25fec/frame.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `value` | `u16` | The decoded value after error correction (for Golay and Hamming blocks) or as received (the raw block); fits 16 bits. |
| `distance` | `u32` | Number of corrected bits; always 0 for the unprotected raw field. |

## `Ratet27Frame` [@ANCHOR: ham_digital_modes:frame_ratet27_frame_struct]

**Purpose.** Every one of RATET(27)'s 8 confirmed sub-blocks, decoded from one 144-bit channel frame.

**Definition.** `pub struct Ratet27Frame` (struct) in `daemons/ham_digital_modes/src/ambe/dvsi_p25fec/frame.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `g0` | `DecodedBlock` | Golay(23,12) block 0 (strongest protection), decoded. |
| `g1` | `DecodedBlock` | Golay(23,12) block 1, decoded. |
| `g2` | `DecodedBlock` | Golay(23,12) block 2, decoded. |
| `g3` | `DecodedBlock` | Golay(23,12) block 3, decoded. |
| `u4` | `DecodedBlock` | Hamming(15,11) block 4, decoded. |
| `u5` | `DecodedBlock` | Hamming(15,11) block 5, decoded. |
| `u6` | `DecodedBlock` | Hamming(15,11) block 6, decoded. |
| `c7` | `DecodedBlock` | The 7 unprotected raw bits (distance always 0). |

**Notes.** Value type (`Copy`); produced by the RATET(27) frame decoder from one 144-bit channel frame, which is the DVSI chip's P25 half-rate wire format. Read-only evidence for tests and tools; this crate does not transmit with it.

## `Block` [@ANCHOR: ham_digital_modes:wire_format_block_enum]

**Purpose.** One of the 8 FEC sub-blocks that RATET(27)'s 144 bits split into, in natural (pre-interleave) order, ordered by decreasing error-protection strength per DVSI's own manual.

**Definition.** `pub enum Block` (enum) in `daemons/ham_digital_modes/src/ambe/dvsi_p25fec/wire_format.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Golay` | `{ index: u8 }` | Golay(23,12), one of `g0..g3` (`index` 0..4). |
| `Hamming` | `{ index: u8 }` | Hamming(15,11), one of `u4..u6` (`index` 0..3). |
| `Raw` | - | The 7 unprotected raw bits. |

**Used by.** `block_for_natural`, `block_wire_members`, `natural_range` (functions whose signature names it).

**Notes.** Used to name which sub-block of a RATET(27) frame is meant (its FEC code and position) when mapping channel bits to values. A value type with no state.

## `DequantizedFrame` [@ANCHOR: ham_digital_modes:decode_dequantized_frame_enum]

**Purpose.** The fixed-point equivalent of `ambe::float::ambe_plus_2::decode::DequantizedFrame`.

**Definition.** `pub enum DequantizedFrame` (enum) in `daemons/ham_digital_modes/src/ambe/fixed/ambe_plus_2/decode.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Speech` | `(SpeechParameters)` | A normal speech frame; carries the dequantized `SpeechParameters`. |
| `Erasure` | - | `b0=121` or `123` only -- see `FrameKind::Erasure`'s own doc comment (float sibling). |
| `Silence` | - | Silence frame; carries the harmonic count `l` and the pitch `w0_q16` (Q16.16 radians per sample) to synthesize zeros with the right state. |
| `Tone` | - | `b0` in `{120, 122, 126, 127}` -- decode the actual tone/digit via `decode_tone_idx(d)` on the frame's own `d`, exactly as the float sibling's own `DequantizedFrame::Tone` doc comment describes (that decode is already pure integer and needs no fixed-point port). |

**Used by.** `dequantize` (functions whose signature names it).

**Notes.** Result of `dequantize`; the fixed-point sibling of the float enum of the same name. In the D-STAR variant the tone case carries a `TonePayload` instead of the raw parameters.

## `Encoder` [@ANCHOR: ham_digital_modes:encoder_encoder_struct]

**Purpose.** Streaming fixed-point PCM-to-frame encoder for this mode: push 16-bit PCM samples, pull one 72-bit logical frame (held in a `u128`) per 20 ms. Defined in this mode's `encoder.rs`; the float sibling has the same public surface.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/ambe_plus_2/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `analyzer` | `FrameAnalyzer` | The `FrameAnalyzer` that buffers input samples and produces the pitch and spectral analysis of each frame. |
| `high_pass` | `HighPassFilter` | The `HighPassFilter` applied to the input before analysis. |
| `mirror` | `MbeDecoderState` | The encoder's own copy of the decoder state (`MbeDecoderState`), advanced with every frame it emits, so quantization predicts from exactly what a decoder will hold. |
| `analysis` | `AnalysisState` | The voicing/amplitude analysis state carried from frame to frame (`AnalysisState`). |

**Notes.** Not shared between threads; `push_samples` and `next_frame` take `&mut self`. `finish` flushes the frames still buffered.

## `AmbePlus2SynthesisDecoder` [@ANCHOR: ham_digital_modes:synthesis_ambe_plus2_synthesis_decoder_struct]

**Purpose.** Streaming fixed-point AMBE+2 half-rate frame-to-PCM decoder: `decode_frame` takes one logical frame and returns 20 ms of Q16.16 PCM (pulse-code modulation).

**Definition.** `pub struct AmbePlus2SynthesisDecoder` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/ambe_plus_2/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `dequant` | `MbeDecoderState` | Decoder state carried across frames (`MbeDecoderState`). |
| `synth` | `MbeSynthesizer` | The shared MBE synthesizer (`MbeSynthesizer`). |
| `tone` | `ToneSynthesizer` | The tone generator used for tone frames (`ToneSynthesizer`). |
| `repeats` | `u32` | Consecutive frames repeated because of channel errors (mbelib's `repeat`). |
| `error_policy` | `ErrorPolicy` | How bad frames are handled (`ErrorPolicy`; default as in the float decoder, settable with `with_error_policy`). |
| `concealer` | `Concealer` | The error concealer that decides between accepting a decoded frame and repeating the previous one (`Concealer`, parameters set with `with_conceal_params`). |

**Notes.** One decoder per stream; `decode_frame` takes `&mut self`.

## `ToneKind` [@ANCHOR: ham_digital_modes:decode_tone_kind_enum]

**Purpose.** The fixed-point equivalent of `ambe::float::dstar::decode::ToneKind`.

**Definition.** `pub enum ToneKind` (enum) in `daemons/ham_digital_modes/src/ambe/fixed/dstar/decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Invalid` | - | Tone index that is not a valid tone. |
| `Single` | - | `hz_q16` is exact, not approximated -- `index * HZ_PER_INDEX_Q16_16` has no rounding error beyond `HZ_PER_INDEX_Q16_16`'s own (exact) representation of `31.25`. |
| `Dual` | - | A two-frequency tone (DTMF-like). |

**Used by.** `classify_tone_index` (functions whose signature names it).

## `DequantizedFrame` [@ANCHOR: ham_digital_modes:dstar_decode_dequantized_frame_enum]

**Purpose.** The fixed-point equivalent of `ambe::float::dstar::decode::DequantizedFrame`.

**Definition.** `pub enum DequantizedFrame` (enum) in `daemons/ham_digital_modes/src/ambe/fixed/dstar/decode.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Speech` | `(SpeechParameters)` | A normal speech frame; carries the dequantized `SpeechParameters`. |
| `Tone` | `(TonePayload)` | A tone frame; carries the `TonePayload` (tone kind, frequencies and level) read from the frame. |

**Used by.** `dequantize` (functions whose signature names it).

**Notes.** Result of the D-STAR `dequantize`; the fixed-point sibling of the float enum of the same name.

## `Encoder` [@ANCHOR: ham_digital_modes:dstar_encoder_encoder_struct]

**Purpose.** Streaming fixed-point PCM-to-frame encoder for D-STAR: push 16-bit PCM samples, pull one 72-bit logical frame (held in a `u128`) per 20 ms. Defined in `dstar/encoder.rs`; the float sibling has the same public surface.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/dstar/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `analyzer` | `FrameAnalyzer` | The `FrameAnalyzer` that buffers input samples and produces the pitch and spectral analysis of each frame. |
| `high_pass` | `HighPassFilter` | The `HighPassFilter` applied to the input before analysis. |
| `mirror` | `MbeDecoderState` | The encoder's own copy of the decoder state (`MbeDecoderState`), advanced with every frame it emits, so quantization predicts from exactly what a decoder will hold. |
| `analysis` | `AnalysisState` | The voicing/amplitude analysis state carried from frame to frame (`AnalysisState`). |

**Notes.** Not shared between threads; the methods take `&mut self`.

## `DStarSynthesisDecoder` [@ANCHOR: ham_digital_modes:synthesis_dstar_synthesis_decoder_struct]

**Purpose.** Streaming fixed-point D-STAR frame-to-PCM decoder: `decode_frame` takes one logical frame and returns 20 ms of Q16.16 PCM.

**Definition.** `pub struct DStarSynthesisDecoder` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/dstar/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `dequant` | `MbeDecoderState` | Decoder state carried across frames (`MbeDecoderState`). |
| `synth` | `MbeSynthesizer` | The shared MBE synthesizer (`MbeSynthesizer`). |
| `tone` | `ToneSynthesizer` | The tone generator used for tone frames (`ToneSynthesizer`). |
| `repeats` | `u32` | Consecutive frames repeated because of channel errors (mbelib's `repeat`). |
| `error_policy` | `ErrorPolicy` | How bad frames are handled (`ErrorPolicy`). |
| `concealer` | `Concealer` | The error concealer (`Concealer`). |

**Notes.** One decoder per stream; the methods take `&mut self`.

## `Descriptor` [@ANCHOR: ham_digital_modes:concealment_descriptor_struct]

**Purpose.** What the continuity prior looks at, from one decoded frame (fixed-point sibling of `Descriptor`).

**Definition.** `pub struct Descriptor` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/concealment.rs`; derives `Clone`, `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `w0_q16` | `i32` | Fundamental frequency (pitch), Q16.16 radians per sample. |
| `level_db_q16` | `i64` | Mean level of the harmonic amplitudes, in dB, Q16.16 (`i64`). |
| `spectrum_db_q16` | `[i64; 16]` | Log amplitude in dB at 16 points evenly spaced in relative frequency, Q16.16. |
| `voicing` | `[bool; 8]` | Voiced flag of each of 8 evenly spaced frequency bands. |

**Used by.** `continuity_cost_q16`, `decide` (functions whose signature names it).

**Notes.** Built from a decoded frame and kept as the "previous frame" in the `Concealer`; the float sibling documents the formulas.

## `ConcealParams` [@ANCHOR: ham_digital_modes:concealment_conceal_params_struct]

**Purpose.** Tunable constants in fixed point; `Default` equals the float `ConcealParams::default()` (checked by a test).

**Definition.** `pub struct ConcealParams` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/concealment.rs`; derives `Clone`, `Copy`, `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `level_scale_db_q16` | `i64` | Laplace scale of the level change between frames, dB, Q16.16. |
| `shape_scale_db_q16` | `i64` | Laplace scale of the spectral shape change (RMS over 16 points), dB, Q16.16. |
| `voicing_cost_q16` | `i64` | Cost per voicing band that differs from the previous frame, nats, Q16.16. |
| `pitch_scale_q16` | `i64` | Laplace scale of the log pitch change when both frames are voiced, Q16.16. |
| `garbage_cost_q16` | `i64` | Cost in nats that "this frame is garbage", before adding the cost of an error having happened at all, Q16.16. |
| `error_suspicion_q16` | `i64` | Extra cost per corrected error the channel code reported (suspicion of a miscorrection), nats, Q16.16. |
| `ber_alpha_q16` | `i64` | Smoothing factor of the bit error rate estimate, Q16.16. |
| `ber_floor_q32` | `i64` | Bit error rate floor, Q32. |
| `flip_gate_q16` | `i64` | Bit corrections are considered only once the estimated rate exceeds the floor times this factor, Q16.16. |
| `fade_per_frame_q16` | `i64` | Amplitude factor applied per consecutive repeated frame, Q16.16. |
| `reset_after` | `u32` | Repeated frames in a row after which the decoder state is reset. |
| `widen_per_repeat_q16` | `i64` | How much the continuity priors widen per consecutive repeated frame, Q16.16. |

**Used by.** `new` (functions whose signature names it).

## `Decision` [@ANCHOR: ham_digital_modes:concealment_decision_enum]

**Purpose.** Decision for one frame.

**Definition.** `pub enum Decision` (enum) in `daemons/ham_digital_modes/src/ambe/fixed/general/concealment.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Accept` | `(usize)` | Use the candidate frame whose index is given. |
| `Repeat` | - | Repeat the previous frame scaled by this Q16.16 factor (0 = silence); `reset` says to reset the decoder state first. |

**Used by.** `decide` (functions whose signature names it).

## `Concealer` [@ANCHOR: ham_digital_modes:concealment_concealer_struct]

**Purpose.** The error concealer: keeps the last accepted frame's `Descriptor`, a running bit error rate estimate and the repeat count, and decides for each received frame whether to accept a candidate (possibly with bits flipped) or repeat the previous frame. Fixed-point sibling of the float `Concealer`.

**Definition.** `pub struct Concealer` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/concealment.rs`.

| Field | Type | Meaning |
|---|---|---|
| `params` | `ConcealParams` | The tuned constants (public; `Default` equals the float defaults). |
| `previous` | `Option<Descriptor>` | Descriptor of the last accepted frame; `None` at the start and after a reset. |
| `ber_q32` | `i64` | Running estimate of the channel bit error rate, Q32 (`i64`). |
| `repeats` | `u32` | Consecutive frames repeated since the last accepted one. |

**Notes.** One per stream; owned by the synthesis decoder.

## `ModeTables` [@ANCHOR: ham_digital_modes:mbe_encode_mode_tables_struct]

**Purpose.** The per-mode tables the quantizer searches (Q16.16 counterparts of the float `ModeTables`).

**Definition.** `pub struct ModeTables<'a>` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `vuv` | `&'a [[bool; 8]]` | Voiced/unvoiced vector table (8 band flags per entry). |
| `dg_q16` | `&'a [i32]` | Gain-delta (`dg`) table, Q16.16. |
| `prba24_q16` | `&'a [[i32; 3]]` | PRBA 2-4 vector table, 3 values per entry, Q16.16. |
| `prba58_q16` | `&'a [[i32; 4]]` | PRBA 5-8 vector table, 4 values per entry, Q16.16. |
| `lmprbl` | `&'a [[u32; 4]]` | Per-harmonic-count DCT block lengths (4 per entry). |
| `hoc_q16` | `[&'a [[i32; 4]]; 4]` | The four higher-order-coefficient tables (`b5` to `b8`), 4 values per entry, Q16.16. |
| `hoc_b8_even_only` | `bool` | D-STAR `b8` only ever carries even indices (its low bit is always 0). |
| `rho_q16` | `i64` | The decoder's predictor weight in Q16.16 (see `mbe_speech::SpeechTables::rho_q16`). |
| `gamma_scale_q16` | `i64` | The decoder's gain recursion in Q16.16 (see `mbe_speech::SpeechTables`). |
| `gamma_memory_q16` | `i64` | The gain recursion's memory factor, Q16.16. |

**Used by.** `quantize_speech` (functions whose signature names it).

**Notes.** Built per mode by `mode_tables`; borrowed (`'a`) from the mode's static tables.

## `SpeechTarget` [@ANCHOR: ham_digital_modes:mbe_encode_speech_target_struct]

**Purpose.** What the frame's analysis wants the decoder to reproduce. `voiced`, `ml_q16` and `log2_ml_q16` are 1-indexed by harmonic (index 0 unused), length `l + 1`.

**Definition.** `pub struct SpeechTarget<'a>` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Number of harmonics. |
| `vuv_f0_q16` | `i32` | Fundamental for the V/UV slot lookup `jl = floor(h * 16 * f0)`, in cycles/sample, Q16.16. |
| `voiced` | `&'a [bool]` | Voiced flag per harmonic, 1-indexed (index 0 unused), length `l + 1`. |
| `ml_q16` | `&'a [i32]` | Linear amplitudes (Q16.16), used only as V/UV weights. |
| `log2_ml_q16` | `&'a [i32]` | Target log2 amplitudes in the decoder's domain (Q16.16), from [`target_log2_ml_q16`]. |

**Used by.** `quantize_speech` (functions whose signature names it).

## `PrevState` [@ANCHOR: ham_digital_modes:mbe_encode_prev_state_struct]

**Purpose.** The decoder-side state the recursion predicts from (mirrors [`MbeDecoderState`]).

**Definition.** `pub struct PrevState<'a>` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Harmonic count of the previous frame. |
| `log2_ml_q16` | `&'a [i32]` | Previous frame's log2 amplitudes, Q16.16, 1-indexed per harmonic. |
| `gamma_q16` | `i32` | Previous frame's gain state, Q16.16. |

**Used by.** `quantize_speech` (functions whose signature names it).

**Notes.** A borrowed view of the encoder's mirror `MbeDecoderState`.

## `QuantizedSpeech` [@ANCHOR: ham_digital_modes:mbe_encode_quantized_speech_struct]

**Purpose.** The eight quantizer indices `b1` to `b8` the encoder chose for one speech frame: V/UV pattern, gain, PRBA 2-4, PRBA 5-8 and the four HOC blocks. Plain value type.

**Definition.** `pub struct QuantizedSpeech` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_encode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `b1` | `u32` | Voiced/unvoiced vector index. |
| `b2` | `u32` | Gain-delta (`dg`) index. |
| `b3` | `u32` | PRBA 2-4 vector index. |
| `b4` | `u32` | PRBA 5-8 vector index. |
| `b5` | `u32` | Higher-order coefficient block 1 index. |
| `b6` | `u32` | Higher-order coefficient block 2 index. |
| `b7` | `u32` | Higher-order coefficient block 3 index. |
| `b8` | `u32` | Higher-order coefficient block 4 index (D-STAR only uses even values). |

**Used by.** `quantize_speech` (functions whose signature names it).

## `RawSpeechParameters` [@ANCHOR: ham_digital_modes:mbe_speech_raw_speech_parameters_struct]

**Purpose.** The nine raw parameter indices needed by [`dequantize_speech`] -- `b0` itself is not included since the caller has already used it to resolve `l`/`w0` before calling this function.

**Definition.** `pub struct RawSpeechParameters` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_speech.rs`.

| Field | Type | Meaning |
|---|---|---|
| `b1` | `u32` | Voiced/unvoiced vector index. |
| `b2` | `u32` | Gain-delta (`dg`) index. |
| `b3` | `u32` | PRBA 2-4 vector index. |
| `b4` | `u32` | PRBA 5-8 vector index. |
| `b5` | `u32` | Higher-order coefficient block 1 index. |
| `b6` | `u32` | Higher-order coefficient block 2 index. |
| `b7` | `u32` | Higher-order coefficient block 3 index. |
| `b8` | `u32` | Higher-order coefficient block 4 index. |

**Used by.** `dequantize_speech` (functions whose signature names it).

**Notes.** `b0` (pitch and harmonic count) is handled by the caller. Built from the frame's bits and consumed by `dequantize_speech`.

## `SpeechTables` [@ANCHOR: ham_digital_modes:mbe_speech_speech_tables_struct]

**Purpose.** Every mode-specific table [`dequantize_speech`] needs, all already in this module's own Q16.16 convention (generated from the mode's real float tables, not re-derived formulas -- see each mode's own `tables_q16.rs` generator for the exact provenance). `prba24`/`prba58` and the four `hoc_b*` tables are kept as separate fields (not one array) since their own row widths differ (3, 4, and 4 columns respectively).

**Definition.** `pub struct SpeechTables<'a>` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_speech.rs`.

| Field | Type | Meaning |
|---|---|---|
| `vuv` | `&'a [[bool; 8]]` | Voiced/unvoiced vector table (8 band flags per entry). |
| `dg_q16` | `&'a [i32]` | Gain-delta (`dg`) table, Q16.16. |
| `prba24_q16` | `&'a [[i32; 3]]` | PRBA 2-4 table, 3 values per row. |
| `prba58_q16` | `&'a [[i32; 4]]` | PRBA 5-8 table, 4 values per row. |
| `lmprbl` | `&'a [[u32; 4]]` | `LMPRBL[l]`'s own four per-block DCT lengths -- pure integers in every mode, reused as-is. |
| `hoc_b5_q16` | `&'a [[i32; 4]]` | HOC block `b5` table, 4 values per row. |
| `hoc_b6_q16` | `&'a [[i32; 4]]` | HOC block `b6` table, 4 values per row. |
| `hoc_b7_q16` | `&'a [[i32; 4]]` | HOC block `b7` table, 4 values per row. |
| `hoc_b8_q16` | `&'a [[i32; 4]]` | HOC block `b8` table, 4 values per row. |
| `rho_q16` | `i64` | The amplitude predictor's weight in Q16.16: `POINT_65_Q16_16` for AMBE+2, `POINT_80_Q16_16` for D-STAR (the chip's value). |
| `gamma_scale_q16` | `i64` | The gain recursion `gamma = scale*DG + memory*gamma_prev` in Q16.16 (`float::dstar::decode::GAMMA_SCALE`). |
| `gamma_memory_q16` | `i64` | Weight of the previous gain in the gain recursion, Q16.16. |

**Used by.** `dequantize_speech` (functions whose signature names it).

**Notes.** Borrowed static tables of one mode (AMBE+2 or D-STAR); not modified.

## `MbeDecoderState` [@ANCHOR: ham_digital_modes:mbe_speech_mbe_decoder_state_struct]

**Purpose.** Persistent decoder state across frames, generic over every MBE-family mode -- the fixed-point equivalent of `ambe_plus_2::decode::DecoderState`/`dstar::decode::DStarDecoderState` (which are themselves identical in shape, just duplicated per mode in the float tree).

**Definition.** `pub struct MbeDecoderState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_speech.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Harmonic count of the last frame. |
| `log2_ml_q16` | `Vec<i32>` | Last frame's log2 amplitudes, Q16.16, 1-indexed per harmonic. |
| `gamma_q16` | `i32` | Last frame's gain state, Q16.16. |

**Used by.** `dequantize_speech` (functions whose signature names it).

**Notes.** `MbeDecoderState::initial()` is the state at stream start; each `dequantize` call updates it, so decoding is order dependent and one state belongs to one stream.

## `SpeechParameters` [@ANCHOR: ham_digital_modes:mbe_speech_speech_parameters_struct]

**Purpose.** One frame's own real, dequantized semantic parameters -- the `FrameKind::Speech` case only (the caller handles `Erasure`/`Silence`/`Tone`/mode-specific special frames itself).

**Definition.** `pub struct SpeechParameters` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_speech.rs`.

| Field | Type | Meaning |
|---|---|---|
| `l` | `u32` | Number of harmonics. |
| `w0_q16` | `i32` | Fundamental frequency, Q16.16 radians per sample. |
| `w0_q32` | `i64` | `w0` at Q32 radians/sample (`2^32` per radian), the precision synthesis needs -- see `voiced_synthesis`. `w0_q16` stays for everything that only needs Q16.16 (amplitude scaling). |
| `voiced` | `Vec<bool>` | Voiced flag per harmonic (1-indexed). |
| `ml_q16` | `Vec<i32>` | Harmonic amplitudes, Q16.16 (1-indexed). |

**Used by.** `dequantize_speech` (functions whose signature names it).

## `MbeSynthesizer` [@ANCHOR: ham_digital_modes:mbe_synthesis_mbe_synthesizer_struct]

**Purpose.** The mode-independent fixed-point MBE frame synthesizer that D-STAR and AMBE+2 share: it turns one dequantized frame into 20 ms (`N` samples) of Q16.16 PCM.

**Definition.** `pub struct MbeSynthesizer` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/mbe_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `synthesis` | `SynthesisState` | The underlying `SynthesisState` (enhancement, smoothing, voiced and unvoiced synthesis). |
| `error_rate_prev_q16` | `i32` | Previous frame's error-rate estimate, Q16.16, for the smoothing thresholds. |
| `lift_history` | `[i64; 2]` | The last two output samples' state for the high-frequency lift filter. |

**Notes.** One per stream; `synthesize_speech`, `synthesize_repeat` and `synthesize_repeat_scaled` take `&mut self`.

## `DetectedTone` [@ANCHOR: ham_digital_modes:tone_detect_detected_tone_enum]

**Purpose.** What the tone detector found in 20 ms of audio: a DTMF digit or a single tone.

**Definition.** `pub enum DetectedTone` (enum) in `daemons/ham_digital_modes/src/ambe/fixed/general/tone_detect.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Dtmf` | - | A DTMF pair: `row` and `col` index the low and high frequency groups. |
| `Single` | - | `index = round(f / 31.25)`, `hz_q16` the measured frequency (Q16.16 Hz). |

## `Detection` [@ANCHOR: ham_digital_modes:tone_detect_detection_struct]

**Purpose.** One tone detection result: which tone and how strong.

**Definition.** `pub struct Detection` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/tone_detect.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `tone` | `DetectedTone` | The detected tone. |
| `amplitude_q16` | `i64` | Per-tone peak amplitude in PCM units, Q16.16 (`i64`). |

**Used by.** `detect_tone` (functions whose signature names it).

## `ToneSynthesizer` [@ANCHOR: ham_digital_modes:tone_synthesis_tone_synthesizer_struct]

**Purpose.** Two phase accumulators (turns); a tone uses one or both.

**Definition.** `pub struct ToneSynthesizer` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/tone_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `phase` | `[u32; 2]` | Two phase accumulators in turns (`u32`, full range is one cycle); a tone uses one or both. |

**Notes.** `synthesize`, `synthesize_hz` and `dtmf` advance the phases so a tone is continuous across frames; `reset` zeroes them.

## `ComplexQ16` [@ANCHOR: ham_digital_modes:unvoiced_synthesis_complex_q16_struct]

**Purpose.** A spectral value at `i64` Q16.16 precision -- see this module's own doc comment for why the ordinary `i32` Q16.16 convention isn't wide enough here.

**Definition.** `pub(crate) struct ComplexQ16` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/unvoiced_synthesis.rs`; derives `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `re` | `i64` | Real part, Q16.16 in an `i64`. |
| `im` | `i64` | Imaginary part, Q16.16 in an `i64`. |

**Used by.** `unvoiced_dft_q16`, `unvoiced_spectrum_q16`, `unvoiced_time_domain_q16` (functions whose signature names it).

**Notes.** Crate-private working type of the unvoiced-synthesis FFT (fast Fourier transform).

## `NoiseState` [@ANCHOR: ham_digital_modes:unvoiced_synthesis_noise_state_struct]

**Purpose.** Persistent state for the noise generator `u(n)` -- see the float sibling's own doc comment for the full derivation; this is a verbatim duplicate (not a numeric port) since the recurrence was already pure integer.

**Definition.** `pub struct NoiseState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/unvoiced_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `last` | `i64` | The last noise value of the recurrence. |
| `window` | `VecDeque<i64>` | The recent values the recurrence needs, oldest first. |

**Used by.** `synthesize`, `unvoiced_dft_q16`, `unvoiced_spectrum_q16` (functions whose signature names it).

## `UnvoicedState` [@ANCHOR: ham_digital_modes:unvoiced_synthesis_unvoiced_state_struct]

**Purpose.** Persistent unvoiced-synthesis state: the previous frame's own time-domain unvoiced signal, for Eq. 126's own overlap-add. See the float sibling's own doc comment for why the noise generator itself is owned by the caller, shared with voiced synthesis, not by this struct.

**Definition.** `pub struct UnvoicedState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/unvoiced_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `previous_time_domain_q16` | `[i64; 256]` | The previous frame's unvoiced time-domain signal (256 samples, Q16.16), for the overlap-add of the next frame. |

## `VoicedState` [@ANCHOR: ham_digital_modes:voiced_synthesis_voiced_state_struct]

**Purpose.** Persistent voiced-synthesis state -- see this module's own doc comment for why `psi`/`phi` are stored as phases, and the float sibling's own doc comment for the full Annex A initial-value derivation (`M_bar_l(-1) = 0.0`, `v_bar_l(-1) = false`, `L~(-1) = 30`, all reused unchanged here).

**Definition.** `pub struct VoicedState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/general/voiced_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `psi_phase` | `[u32; MAX_HARMONICS]` | Per-harmonic oscillator phase, as a `u32` turn fraction. |
| `phi_phase` | `[u32; MAX_HARMONICS]` | Per-harmonic phase-offset state, as a `u32` turn fraction. |
| `omega0_prev_q32` | `i64` | Previous frame's fundamental, Q32 radians per sample. |
| `l_hat_prev` | `u32` | Previous frame's harmonic count. |
| `voiced_prev` | `[bool; MAX_HARMONICS]` | Previous frame's voiced flag per harmonic. |
| `amplitudes_prev_q16` | `[i32; MAX_HARMONICS]` | Previous frame's amplitude per harmonic, Q16.16. |

**Notes.** Array length is `MAX_HARMONICS`; initial values follow Annex A (amplitudes 0, unvoiced, 30 harmonics).

