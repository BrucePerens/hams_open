# Fixed-point TIA-102.BABA data structures (ambe/fixed/tia_102_baba, mbe_encode)

As a developer changing this code, I want every data structure in `src/ambe/fixed/mbe_encode.rs`, `src/ambe/fixed/tia_102_baba/decode.rs`, `src/ambe/fixed/tia_102_baba/encode.rs`, `src/ambe/fixed/tia_102_baba/encoder.rs`, `src/ambe/fixed/tia_102_baba/error_estimation.rs`, `src/ambe/fixed/tia_102_baba/pitch.rs`, `src/ambe/fixed/tia_102_baba/pitch_refinement.rs` and `src/ambe/fixed/tia_102_baba/synthesis.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `AnalysisState` [@ANCHOR: ham_digital_modes:mbe_encode_analysis_state_struct]

**Purpose.** Frame-to-frame state of the voicing/amplitude analysis (energy tracker, `Q16`, and the previous frame's band decisions).

**Definition.** `pub struct AnalysisState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/mbe_encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `xi_max_q16` | `i128` | The running maximum energy tracker of the voicing/amplitude analysis, Q16 in an `i128`. |
| `prev_bands` | `Vec<bool>` | The previous frame's voiced/unvoiced decision for each frequency band. |

**Used by.** `analyze_and_quantize`, `analyze_at_pitch`, `analyze_frame_at_pitch` (functions whose signature names it).

**Notes.** Starts from `XI_MAX_INITIAL_Q16` and an empty band list (`AnalysisState::new`); updated by every frame the encoder analyses, one per stream.

## `DecodedParameters` [@ANCHOR: ham_digital_modes:decode_decoded_parameters_struct]

**Purpose.** The fixed-point equivalent of `DecodedParameters`.

**Definition.** `pub struct DecodedParameters` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `omega0_tilde_q16` | `i32` | Dequantized fundamental frequency, Q16.16 radians per sample. |
| `omega0_tilde_q32` | `i64` | `omega0_tilde` at Q32 radians/sample -- the precision synthesis uses. |
| `l_hat` | `u32` | Number of harmonics of the frame (`L_hat`). |
| `k_hat` | `u32` | Number of higher-order-coefficient blocks derived from it (`K_hat`). |
| `bits` | `DeprioritizedBits` | The de-prioritized bit groups of the frame (`DeprioritizedBits`). |
| `voiced` | `Vec<bool>` | Voiced flag per harmonic. |
| `reconstructed_amplitudes_q16` | `Vec<i32>` | Reconstructed spectral amplitudes `M~_l`, Q16.16 (before enhancement). |
| `errors` | `FrameErrorsQ16` | The frame's channel error statistics (`FrameErrorsQ16`). |

**Used by.** `advance_history` (functions whose signature names it).

**Notes.** Everything a non-repeat frame decode recovers, exposed so a caller (notably the round-trip tests) can check exact parameter fidelity, not only that synthesis produced PCM.

## `FrameOutcome` [@ANCHOR: ham_digital_modes:decode_frame_outcome_enum]

**Purpose.** The fixed-point equivalent of `FrameOutcome`.

**Definition.** `pub enum FrameOutcome` (enum) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/decode.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Repeat` | - | The frame is a section 7.7 repeat: reuse the previous real frame's parameters. |
| `Mute` | - | A section 7.8 mute: synthesize comfort noise instead of speech. |
| `Decoded` | `(DecodedParameters)` | A genuine decode, with the recovered parameters. |

**Notes.** The fixed-point sibling of the float `FrameOutcome`; both repeat and mute update the carried history the same way and differ in what is synthesized.

## `DecoderState` [@ANCHOR: ham_digital_modes:decode_decoder_state_struct]

**Purpose.** The fixed-point equivalent of `DecoderState`. `error_rate_prev` is `i32` Q16.16, matching `estimate_errors_q16`'s own domain; `spectral_amplitudes_prev` is `Vec<i32>` Q16.16, seeded to [`ONE_Q16_16`] per Annex A, matching the float sibling's own `1.0`.

**Definition.** `pub struct DecoderState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `synthesis` | `SynthesisState` | The shared `SynthesisState` (noise generator, voiced and unvoiced synthesis, enhancement state, last frame's synthesis inputs). |
| `l_hat_prev` | `u32` | Harmonic count of the previous real frame (initially 30, Annex A). |
| `spectral_amplitudes_prev` | `Vec<i32>` | The previous frame's unenhanced amplitudes `M~_l(-1)`, Q16.16 (initially unity per Annex A). |
| `error_rate_prev_q16` | `i32` | The running error-rate average `epsilon_R`, Q16.16 (initially 0). |
| `chip_wire` | `bool` | Decode the DVSI chip's framing instead of the standard's (see the float sibling's `new_chip_wire`). |

**Notes.** One per stream; decoding is order dependent and mutates this state.

## `FrameState` [@ANCHOR: ham_digital_modes:encode_frame_state_struct]

**Purpose.** The per-frame state carried into the next [`encode_frame`] call: the energy tracker, the previous frame's band decisions and harmonic count, and its reconstructed amplitudes (Q16.16), i.e. what the decoder will hold.

**Definition.** `pub struct FrameState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/encode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `xi_max_q16` | `i128` | The energy tracker (`xi_max`), Q16 in an `i128`. |
| `l_hat` | `u32` | The previous frame's harmonic count. |
| `voiced` | `Vec<bool>` | The previous frame's voiced flag per harmonic. |
| `spectral_amplitudes_q16` | `Vec<i32>` | The previous frame's reconstructed amplitudes, Q16.16: what the decoder will hold. |

**Used by.** `encode_frame`, `encode_prioritized_bits` (functions whose signature names it).

**Notes.** Carried by the caller from one `encode_frame` call to the next; one per stream.

## `FrameAnalysis` [@ANCHOR: ham_digital_modes:encoder_frame_analysis_struct]

**Purpose.** Everything downstream stages need for one frame.

**Definition.** `pub struct FrameAnalysis` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `p8` | `u32` | Refined pitch period in eighths of a sample (`P = p8 / 8`). |
| `pitch` | `Pitch` | The refined pitch as an exact fraction plus `omega0` (Q30); see [`Pitch`]. |
| `initial_pitch_error_q16` | `i32` | `E(P_hat_I)` of the initial pitch estimate, Q16.16 (Eq. 37's `E(P_hat_I)`). |
| `refinement` | `RefinementFrame` | The refinement spectrum `S_w(m)` of the frame. |
| `slot_samples` | `Vec<i32>` | The frame's own 160 input samples (for tone detection). |

**Used by.** `next_analysis` (functions whose signature names it).

**Notes.** Produced by `FrameAnalyzer::next_analysis` for each 160-sample slot and consumed by `encode_frame`.

## `FrameAnalyzer` [@ANCHOR: ham_digital_modes:encoder_frame_analyzer_struct]

**Purpose.** Streaming analyzer; feed PCM with [`Self::push_samples`], pull frames with [`Self::next_analysis`].

**Definition.** `pub struct FrameAnalyzer` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `raw` | `Vec<i32>` | Sliding buffer of the input samples as `i32` (the first `trimmed` samples of the stream have been discarded from it). |
| `trimmed` | `usize` | Number of leading stream samples already discarded from `raw`. |
| `real_samples` | `usize` | Number of samples pushed by the caller so far (silence added at `finish` is not counted). |
| `next_frame` | `usize` | Index of the next frame to analyse. |
| `center_offset` | `i32` | Shift of the analysis window against the frame slot in samples, clamped to -160..160 (`set_center_offset`). |
| `prev_first` | `(usize, i32)` | Look-back state for pitch tracking: the previous frame's chosen pitch candidate index and its associated error value (seeded with `DEFAULT_PITCH_INDEX`, 0). |
| `prev_second` | `(usize, i32)` | The same pair for the frame before that. |
| `tables` | `VecDeque<(usize, ErrorTable)>` | Pitch error tables of recent frames, oldest first, as (frame index, `ErrorTable`). |
| `finished` | `bool` | Set by `finish`; after it the analyzer drains the remaining frames against added silence. |

**Notes.** One per stream; takes `&mut self`. Two frames of look-ahead are needed before a frame is analysed.

## `HighPassFilter` [@ANCHOR: ham_digital_modes:encoder_high_pass_filter_struct]

**Purpose.** Streaming fixed-point TIA-102.BABA encoder: fixed-point sibling of the float `tia_102_baba::encoder::Encoder` (same surface, 16-bit PCM in, `[u32; 8]` code vectors out), composing [`FrameAnalyzer`] with [`super::encode::encode_frame`]. Two frames of look-ahead delay; [`Encoder::finish`] flushes with silence. The standard's input high-pass filter (Eq. 3, `H(z) = (1 - z^-1) / (1 - 0.99 z^-1)`), in integer arithmetic: the recursive state is kept in Q16 with the pole in Q30, so the output is the exact filter response rounded to an integer sample without accumulating rounding error. Fixed-point sibling of the float `tia_102_baba::encoder::HighPassFilter`.

**Definition.** `pub struct HighPassFilter` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/encoder.rs`; derives `Clone`, `Copy`, `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `prev_input` | `i64` | The previous input sample. |
| `prev_output_q16` | `i64` | The previous output, Q16. |

**Notes.** `Default` is the all-zero start state; `Copy`, so a filter can be saved and restored.

## `Encoder` [@ANCHOR: ham_digital_modes:tia_102_baba_encoder_encoder_struct]

**Purpose.** Streaming fixed-point TIA-102.BABA encoder, sibling of the float `Encoder`: 16-bit PCM in, `[u32; 8]` code vectors out. It composes the high-pass filter, `FrameAnalyzer` and `encode_frame`, with two frames of look-ahead delay; `finish` flushes with silence.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `high_pass` | `HighPassFilter` | The input high-pass filter. |
| `analyzer` | `FrameAnalyzer` | The frame analyzer. |
| `state` | `FrameState` | The `FrameState` carried between `encode_frame` calls. |
| `last_frame` | `Option<[u32; 8]>` | The last frame emitted, repeated when a later analysis fails. |
| `failed_frames` | `usize` | Frames for which analysis failed (degenerate pitch/`L_hat`) and the previous frame was repeated. |

**Notes.** One per stream; takes `&mut self`.

## `FrameErrorsQ16` [@ANCHOR: ham_digital_modes:error_estimation_frame_errors_q16_struct]

**Purpose.** The fixed-point equivalent of `FrameErrors` -- `total`/`golay_init`/`hamming_init` are already plain integer counts in the float sibling, unchanged here; only `rate` becomes Q16.16.

**Definition.** `pub struct FrameErrorsQ16` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/error_estimation.rs`.

| Field | Type | Meaning |
|---|---|---|
| `total` | `u32` | Total number of corrected channel errors in the frame. |
| `rate_q16` | `i32` | Error rate of the frame, Q16.16. |
| `golay_init` | `u32` | Errors corrected in the initial Golay vectors. |
| `hamming_init` | `u32` | Errors corrected in the initial Hamming vectors. |

**Used by.** `should_mute_frame_q16`, `should_repeat_frame_q16` (functions whose signature names it).

## `PitchAnalysisFrame` [@ANCHOR: ham_digital_modes:pitch_pitch_analysis_frame_struct]

**Purpose.** One frame's pitch-analysis state: the normalised lowpass-filtered samples, `S`, and the integer-lag autocorrelation of `s * w^2`.

**Definition.** `pub struct PitchAnalysisFrame` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/pitch.rs`.

| Field | Type | Meaning |
|---|---|---|
| `s_energy` | `i64` | `sum (s*w)^2` over the 301-tap window (block-normalised units). |
| `autocorr` | `[i64; 151]` | `R[t]`, `t = 0..=150`. |
| `silent` | `bool` | True when the frame has no energy; no pitch is searched for it. |

## `ErrorTable` [@ANCHOR: ham_digital_modes:pitch_error_table_struct]

**Purpose.** `E(P)` (Q16.16) for the 203 candidate pitches of one frame.

**Definition.** `pub struct ErrorTable(pub [i32; CANDIDATES]);` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/pitch.rs`; derives `Clone`.

**Used by.** `error_table`, `look_ahead_pitch_tracking`, `look_back_pitch_tracking` (functions whose signature names it).

## `Cplx` [@ANCHOR: ham_digital_modes:pitch_refinement_cplx_struct]

**Purpose.** A complex number with `i64` parts; the scale is stated by whoever holds it.

**Definition.** `pub struct Cplx` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/pitch_refinement.rs`; derives `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `re` | `i64` | Real part (scale stated by the holder). |
| `im` | `i64` | Imaginary part (scale stated by the holder). |

**Used by.** `harmonic_amplitude_q16`, `sw_at` (functions whose signature names it).

## `Pitch` [@ANCHOR: ham_digital_modes:pitch_refinement_pitch_struct]

**Purpose.** A fundamental frequency in the form the band arithmetic needs: `u = 256 * omega0 / (2 * pi)`, the spacing of the harmonics in `S_w` bins, as an exact fraction `n / d`, plus `omega0` itself in Q30.  Every band edge is `a_l = (l - 1/2) u` and every window index `floor(64 (m - l u) + 1/2)`, so with `u = n / d` both are exact integer divisions. From a period in eighths of a sample ([`Pitch::from_p8`]) `u = 2048 / P8` is exact (no `pi` at all); from an arbitrary `omega0` ([`Pitch::from_omega0_q30`]) `u = omega0 * (256 / 2 pi)` is formed with a Q40 constant, an integer product with no rounding beyond that of `omega0` itself.

**Definition.** `pub struct Pitch` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/pitch_refinement.rs`; derives `Clone`, `Copy`, `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `n` | `i128` | Numerator of `u`, the harmonic spacing in `S_w` bins, as an exact fraction `n / d`. |
| `d` | `i128` | Denominator of `u`. |
| `omega0_q30` | `i64` | The fundamental `omega0` in Q30 radians per sample. |

**Used by.** `harmonic_amplitude_q16`, `refinement_error`, `spectrum_error_and_energy` (functions whose signature names it).

**Notes.** Built by `Pitch::from_p8` (exact, from a period in eighths of a sample) or `Pitch::from_omega0_q30`; `Copy`.

## `RefinementFrame` [@ANCHOR: ham_digital_modes:pitch_refinement_refinement_frame_struct]

**Purpose.** The 256-point windowed DFT of one frame: `S_w(m)` for `m = -127..=128`, Q10.

**Definition.** `pub struct RefinementFrame` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/pitch_refinement.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sw` | `[Cplx; 256]` | The 256 complex bins `S_w(m)`, `m = -127..=128` (index 0 is bin -127), Q10. |

**Used by.** `harmonic_amplitude_q16`, `refine_pitch`, `refinement_error`, `spectrum_error_and_energy` (functions whose signature names it).

## `SynthesisState` [@ANCHOR: ham_digital_modes:synthesis_synthesis_state_struct]

**Purpose.** Persistent decoder-side synthesis state -- the fixed-point equivalent of [`super::super::float::tia_102_baba::synthesis::SynthesisState`]. `s_e`/`tau_m` are `i64` Q16.16, matching [`update_local_energy_q16`]/[`update_amplitude_threshold_q16`]'s own domain (see those functions' own doc comments for why `i32` isn't wide enough). `s_e = 75000.0`, `tau_m = 20480.0` per Annex A / Eq. 115's own first-branch value, exactly as the float sibling's own doc comment derives.

**Definition.** `pub struct SynthesisState` (struct) in `daemons/ham_digital_modes/src/ambe/fixed/tia_102_baba/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `noise` | `NoiseState` | The noise generator (`NoiseState`). |
| `unvoiced` | `UnvoicedState` | Unvoiced synthesis state. |
| `voiced` | `VoicedState` | Voiced synthesis state. |
| `s_e` | `i64` | The local energy `S_E`, Q16.16 in an `i64` (initially 75000.0). |
| `tau_m` | `i64` | The amplitude threshold `tau_M`, Q16.16 in an `i64` (initially 20480.0). |
| `first_frame` | `bool` | True until the first frame has been synthesized. |
| `last_final_amplitudes` | `Option<(i64, Vec<bool>, Vec<i32>)>` | The last real frame's final synthesis inputs (omega0, voicing, amplitudes) kept for a repeat; `None` before the first frame. |
| `comfort_noise_seed` | `i64` | Seed of the comfort-noise generator used by a mute. |
| `unvoiced_gain_q16` | `i64` | Q16.16 multiplier on the unvoiced half; 65536 (exactly 1.0, applied as a no-op) for the standard. |

**Notes.** Fixed-point equivalent of the float `SynthesisState`; one per stream; `unvoiced_gain_q16` is 1.0 (no-op) for the standard.

