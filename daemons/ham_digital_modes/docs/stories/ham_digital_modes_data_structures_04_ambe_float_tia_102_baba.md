# Float TIA-102.BABA data structures (ambe/float/tia_102_baba)

As a developer changing this code, I want every data structure in `src/ambe/float/tia_102_baba/bit_prioritization.rs`, `src/ambe/float/tia_102_baba/decode.rs`, `src/ambe/float/tia_102_baba/encoder.rs`, `src/ambe/float/tia_102_baba/error_estimation.rs`, `src/ambe/float/tia_102_baba/interleave.rs`, `src/ambe/float/tia_102_baba/mod.rs`, `src/ambe/float/tia_102_baba/pitch.rs`, `src/ambe/float/tia_102_baba/pitch_refinement.rs`, `src/ambe/float/tia_102_baba/synthesis.rs`, `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` and `src/ambe/float/tia_102_baba/voiced_synthesis.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `DeprioritizedBits` [@ANCHOR: ham_digital_modes:bit_prioritization_deprioritized_bits_struct]

**Purpose.** [`deprioritize_bits`]'s own decoded parameters, one field per quantizer value [`prioritize_bits`] originally packed together.

**Definition.** `pub struct DeprioritizedBits` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/bit_prioritization.rs`; derives `Debug`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `b0` | `u32` | The pitch quantizer index `b_hat_0`. |
| `b1` | `u32` | The V/UV quantizer index `b_hat_1`. |
| `b2` | `u32` | The gain quantizer index `b_hat_2`. |
| `gain_vector` | `[(u32, u8); 5]` | `b_hat_3..b_hat_7` as five (value, bit width) pairs. |
| `higher_order` | `Vec<(u32, u8)>` | The non-zero-width higher-order coefficients `b_hat_8..` as (value, bit width) pairs. |
| `sync_bit` | `bool` | The frame's sync bit. |

**Notes.** Result of `deprioritize_bits`, the inverse of `prioritize_bits`; plain data (`Debug`, `PartialEq`).

## `DecodedParameters` [@ANCHOR: ham_digital_modes:tia_102_baba_decode_decoded_parameters_struct]

**Purpose.** Everything [`DecoderState::decode_parameters`] recovers from a non-repeat frame, exposed on its own (rather than only fed straight into synthesis) so a caller -- notably this module's own round-trip test -- can assert exact parameter fidelity against what was encoded, not just that synthesis produced *some* finite PCM (pulse-code modulation).

**Definition.** `pub struct DecodedParameters` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `omega0_tilde` | `f64` | Dequantized fundamental frequency `omega0_tilde`, radians per sample. |
| `l_hat` | `u32` | Number of harmonics `L_hat`. |
| `k_hat` | `u32` | Number of frequency bands `K_hat` derived from `L_hat`. |
| `bits` | `DeprioritizedBits` | The de-prioritized bit fields of the frame (`DeprioritizedBits`). |
| `voiced` | `Vec<bool>` | Voiced flag per harmonic. |
| `reconstructed_amplitudes` | `Vec<f64>` | Reconstructed spectral amplitudes `M~_l` (unenhanced). |
| `errors` | `FrameErrors` | The frame's channel error statistics (`FrameErrors`). |

**Used by.** `advance_history` (functions whose signature names it).

## `FrameOutcome` [@ANCHOR: ham_digital_modes:tia_102_baba_decode_frame_outcome_enum]

**Purpose.** The three things a received frame can resolve to before synthesis: a genuine decode, a section 7.7 repeat, or a section 7.8 mute -- the latter two share the *same* "step (2)" parameter-copy update (Eq. 99-104, confirmed directly: 7.8's own text says "first compute the update equations as listed in step (2) of the frame repeat process"), but diverge on what gets synthesized: a repeat reuses the last real frame's own final parameters for actual speech synthesis; a mute bypasses speech synthesis entirely and emits real comfort noise instead ([`SynthesisState::synthesize_comfort_frame`]).

**Definition.** `pub enum FrameOutcome` (enum) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/decode.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Repeat` | - | The frame is a section 7.7 repeat: reuse the previous real frame's parameters. |
| `Mute` | - | A section 7.8 mute: synthesize comfort noise instead of speech. |
| `Decoded` | `(DecodedParameters)` | A genuine decode with the recovered parameters. |

## `DecoderState` [@ANCHOR: ham_digital_modes:tia_102_baba_decode_decoder_state_struct]

**Purpose.** Persistent decoder state across frames: the shared [`SynthesisState`] (noise generator, both synthesis halves, enhancement-stage `S_E`/`tau_M`, and the last real frame's own final synthesis inputs for a repeat), plus the two pieces [`reconstruct_spectral_amplitudes`] itself needs frame to frame -- `l_hat_prev`/`spectral_amplitudes_prev`, the *unenhanced* `M~_l(-1)` history (Eq. 77's own prediction input, a real, separate history from `SynthesisState`'s own enhanced `M_bar_l(-1)`), and `error_rate_prev` (`epsilon_R`, Eq. 96's own running average). Initial values per Annex A: `L~(-1) = 30` ([`INITIAL_L_HAT_PREV`]), `M~_l(-1) = 1` for all `l` (unity, not silent -- matching this codebase's own encoder-side `FrameState::initial()` choice, itself independently confirmed against Annex A when `voiced_synthesis.rs` was built), `epsilon_R(-1) = 0.0`.

**Definition.** `pub struct DecoderState` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `synthesis` | `SynthesisState` | The shared `SynthesisState` (noise generator, both synthesis halves, enhancement state, last real frame's synthesis inputs). |
| `l_hat_prev` | `u32` | Previous real frame's harmonic count (initially 30, Annex A). |
| `spectral_amplitudes_prev` | `Vec<f64>` | The previous frame's unenhanced amplitudes `M~_l(-1)` (initially all 1.0). |
| `error_rate_prev` | `f64` | The running error-rate average `epsilon_R` (initially 0.0). |
| `chip_pitch_map` | `bool` | Use the DVSI chip's log-scale pitch index (accepting `b0` up to 255) instead of the TIA linear one. |
| `chip_wire` | `bool` | Decode the DVSI chip's framing (no demodulation, chip Hamming labelling) instead of the standard's. |
| `forced_l_hat` | `Option<u32>` | Experiments only: when set, every frame is decoded with this many harmonics regardless of `b0`. |
| `l_alpha` | `Option<f64>` | Experiments only: when set, the harmonic count is `round(alpha*pi/omega0)` clamped to 9..=56 instead of Eq. 47. |
| `fade_concealment` | `bool` | Measurement option (float only, off by default): a repeat that fades to 0.8 of the previous level per consecutive damaged frame instead of the standard's flat repeat and comfort-noise mute. |
| `consecutive_repeats` | `u32` | Number of consecutive repeated frames, used for the fade. |

**Notes.** One per stream; decoding mutates it. Public setters: `set_forced_l_hat`, `set_l_alpha`, `set_fade_concealment`; constructors `new` and `new_chip`.

## `ErrorTable` [@ANCHOR: ham_digital_modes:encoder_error_table_struct]

**Purpose.** One frame's error function `E(P)` sampled at every candidate pitch (`21, 21.5, ..., 122`).

**Definition.** `struct ErrorTable(Vec<f64>);` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/encoder.rs`.

**Notes.** Private to `encoder.rs`; one `E(P)` vector per frame, computed by `PitchAnalysisFrame::error_table` and kept in `FrameAnalyzer::tables`.

## `FrameAnalysis` [@ANCHOR: ham_digital_modes:tia_102_baba_encoder_frame_analysis_struct]

**Purpose.** One frame's pitch analysis result: the refined fundamental, the initial estimate's error `E(P_hat_I)`, and the frame's own windowed spectrum ready for voicing/amplitude analysis.

**Definition.** `pub struct FrameAnalysis` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `omega0_hat` | `f64` | The refined fundamental frequency `omega0_hat`, radians per sample. |
| `initial_pitch` | `f64` | The initial pitch estimate `P_hat_I` (samples, before refinement). |
| `initial_pitch_error` | `f64` | `E(P_hat_I)`, the error function value of the initial pitch estimate. |
| `refinement` | `RefinementFrame` | The frame's windowed spectrum `S_w(m)` for voicing and amplitude analysis. |
| `slot_samples` | `Vec<f64>` | The 160 input samples of this frame's own 20 ms slot (`k*160..(k+1)*160`, zero padded past the input's end), for tone detection. |

**Used by.** `next_analysis` (functions whose signature names it).

## `FrameAnalyzer` [@ANCHOR: ham_digital_modes:tia_102_baba_encoder_frame_analyzer_struct]

**Purpose.** Streaming pitch analysis shared by every mode's encoder: buffers input, runs the spec's two-frame lookahead pitch tracking and half/quarter-sample refinement, and yields one [`FrameAnalysis`] per 20 ms frame.

**Definition.** `pub struct FrameAnalyzer` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `raw` | `Vec<f64>` | `LEAD` zeros followed by every input sample received (older samples are trimmed, see `trimmed`). |
| `trimmed` | `usize` | Number of samples already dropped from the front of `raw`. |
| `real_samples` | `usize` | Real (non-padding) samples pushed so far. |
| `next_frame` | `usize` | Index of the next frame to analyse. |
| `center_offset` | `i32` | Shift of the analysis window against the frame slot, in samples (set by `set_center_offset`). |
| `prev1` | `(f64, f64)` | Look-back state for the pitch tracking: the previous frame's chosen pitch and its error. |
| `prev2` | `(f64, f64)` | The same pair for the frame before that. |
| `tables` | `VecDeque<(usize, ErrorTable)>` | Recent frames' `ErrorTable`s as (frame index, table), oldest first, needed for the two-frame lookahead. |
| `finished` | `bool` | Set by `finish`; afterwards the analyzer drains the remaining frames against added silence. |

## `HighPassFilter` [@ANCHOR: ham_digital_modes:tia_102_baba_encoder_high_pass_filter_struct]

**Purpose.** The standard's input high-pass filter (Eq. 3, section 5): `H(z) = (1 - z^-1) / (1 - 0.99 z^-1)`, removing any residual D.C. before analysis. Without it a constant offset in the input (common in recordings) makes quiet frames look strongly periodic at every lag, corrupting the pitch error function `E(P)` (found by comparing against the OP25 `imbe_vocoder` encoder, which applies it; see `docs/references/tia_102_baba_cross_validation.md`).

**Definition.** `pub struct HighPassFilter` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/encoder.rs`; derives `Clone`, `Copy`, `Debug`, `Default`.

| Field | Type | Meaning |
|---|---|---|
| `prev_input` | `i64` | The previous input sample. |
| `prev_output_q16` | `i64` | The previous output, Q16. |

**Notes.** `Default` is the zero state; `Copy`.

## `Encoder` [@ANCHOR: ham_digital_modes:tia_102_baba_encoder_encoder_struct_alt]

**Purpose.** Streaming float TIA-102.BABA encoder: 16-bit PCM in, `[u32; 8]` code vectors out. It composes the high-pass filter, `FrameAnalyzer` and `encode_frame`, with two frames of look-ahead delay; `finish` flushes with silence.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/encoder.rs`.

| Field | Type | Meaning |
|---|---|---|
| `high_pass` | `HighPassFilter` | The input high-pass filter. |
| `analyzer` | `FrameAnalyzer` | The shared frame analyzer. |
| `state` | `FrameState` | The `FrameState` carried between `encode_frame` calls. |
| `last_frame` | `Option<[u32; 8]>` | The last frame emitted, repeated when a later analysis fails. |
| `failed_frames` | `usize` | Frames for which analysis failed (degenerate pitch/`L_hat`) and the previous frame was repeated. |
| `last_analysis` | `Option<(f64, f64, f64)>` | The last frame's initial pitch estimate `P_hat_I`, refined fundamental `omega0_hat` and `E(P_hat_I)`, for diagnostics (`examples/tia_102_baba_oracle_encode_dump.rs`). |
| `chip_wire` | `bool` | Emit the DVSI chip's framing instead of the standard's (see [`super::encode_code_vectors_chip`]). |

**Notes.** One per stream; takes `&mut self`.

## `FrameErrors` [@ANCHOR: ham_digital_modes:error_estimation_frame_errors_struct]

**Purpose.** The per-frame error-tracking state (Eq. 95-96): `total` is `epsilon_T`, this frame's own total corrected-bit-error count across all seven FEC-protected code vectors; `rate` is `epsilon_R(0)`, a slow-moving frame-to-frame average of that count (Eq. 41-style exponential update, unrelated to `vuv::update_xi_max` despite the similar shape -- a coincidence of two different EWMA trackers in this spec, not shared logic); `golay_init` is `epsilon_0`, the error count for `u_hat_0` specifically (the one code vector bit modulation never touches, per `modulation.rs`'s own doc comment, so its own error count is a meaningful signal on its own, not just folded into the total); `hamming_init` is `epsilon_4`, the error count for `u_hat_4` (the first Hamming-coded vector) -- used by `super::enhancement`'s own Eq. 112 threshold, not by anything in this module.

**Definition.** `pub struct FrameErrors` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/error_estimation.rs`.

| Field | Type | Meaning |
|---|---|---|
| `total` | `u32` | `epsilon_T`: total corrected bit errors across the seven FEC-protected code vectors in this frame. |
| `rate` | `f64` | `epsilon_R(0)`: slow-moving frame-to-frame average of `total`. |
| `golay_init` | `u32` | `epsilon_0`: error count of `u_hat_0`, the code vector bit modulation never touches. |
| `hamming_init` | `u32` | `epsilon_4`: error count of `u_hat_4`, the first Hamming-coded vector, used by the enhancement threshold (Eq. 112). |

**Used by.** `should_mute_frame`, `should_repeat_frame` (functions whose signature names it).

## `BitSource` [@ANCHOR: ham_digital_modes:interleave_bit_source_struct]

**Purpose.** One row of Annex H's own table: `(bit1_source, bit0_source)`, where each source is `(vector_index, bit_index)` -- `vector_index` selects which of `c_hat_0..c_hat_7` the bit comes from, and `bit_index` is that vector's own bit position (bit 0 = LSB: lower sideband, matching this codec's existing convention throughout `fec.rs`/`modulation.rs`).

**Definition.** `type BitSource = (u8, u8);` (type) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/interleave.rs`.

## `FrameState` [@ANCHOR: ham_digital_modes:tia_102_baba_frame_state_struct]

**Purpose.** The per-frame state that carries forward into the *next* call to [`encode_frame`] -- Eq. 41's energy tracker (`xi_max`) and Eq. 54's prediction residual (`l_hat`, `voiced`, and each harmonic's own spectral amplitude) are all genuinely stateful across frames, unlike every other stage in this pipeline. `spectral_amplitudes` here is real reconstructed history (via [`reconstruct::reconstruct_spectral_amplitudes`]) for every frame after [`Self::initial`]'s own frame-0 placeholder -- not the current frame's own unquantized estimate, which `encode_frame` only ever uses locally to compute *this* frame's own residual, never stores.  [`Self::initial`] gives this module's own reasoned frame-0 initialization (see its doc comment for exactly which pieces the spec itself dictates and which are our own low-stakes choice); every later frame's state comes from the previous call's own return value. Fields are private -- [`Self::initial`] and `encode_frame`'s own returned state are the only ways to construct or advance one, so a caller can't build a `FrameState` with a `voiced`/`spectral_amplitudes` length that doesn't match its own `l_hat`.

**Definition.** `pub struct FrameState` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/mod.rs`.

| Field | Type | Meaning |
|---|---|---|
| `xi_max` | `f64` | Energy tracker `xi_max` (Eq. 41). |
| `l_hat` | `u32` | Previous frame's harmonic count. |
| `voiced` | `Vec<bool>` | Previous frame's voiced flag per harmonic. |
| `spectral_amplitudes` | `Vec<f64>` | Previous frame's reconstructed amplitudes (the "what the decoder will have" history). |

**Used by.** `encode_frame`, `encode_frame_chip_wire`, `encode_prioritized_bits` (functions whose signature names it).

**Notes.** Fields are private; `FrameState::initial()` and the state returned by `encode_frame` are the only ways to construct or advance one.

## `QuantizedAmplitudes` [@ANCHOR: ham_digital_modes:tia_102_baba_quantized_amplitudes_struct]

**Purpose.** One frame's spectral amplitudes after prediction, block transforms and quantization (Eq. 54-64), and the amplitudes the decoder will reconstruct from them.

**Definition.** `pub struct QuantizedAmplitudes` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/mod.rs`.

| Field | Type | Meaning |
|---|---|---|
| `b2` | `u8` | `b_hat_2`, the gain quantizer index. |
| `gain_vector` | `[(u32, u8); 5]` | `b_hat_3..b_hat_7` with their bit widths. |
| `higher_order` | `Vec<(u32, u8)>` | The non-zero-width higher-order coefficients `b_hat_8..b_hat_{L+1}` with their bit widths. |
| `reconstructed` | `Vec<f64>` | `M_tilde_l` for `1 <= l <= L`: this frame's quantizer values run back through dequantization, the "what the decoder will have" history Eq. 54 needs for the *next* frame (Fig. 16's feedback block). |

**Used by.** `quantize_spectral_amplitudes` (functions whose signature names it).

## `PitchAnalysisFrame` [@ANCHOR: ham_digital_modes:tia_102_baba_pitch_pitch_analysis_frame_struct]

**Purpose.** Per-frame precomputation for `E(P)` (Eq. 5): `s_LPF(j)` for `j` in `-150..=150` (a 21-tap FIR sum per sample, so computed once per frame), the frame energy `sum (s_LPF w_I)^2`, and `r(t)` (Eq. 7) for every integer lag the candidate set can reach. `r(-t) = r(t)` (shift `j` by `t`), so lags `0..=R_LAGS` are enough for every `n * P` in Eq. 5 and the linear interpolation of Eq. 8; tabulating them once turns the 203-candidate error table from roughly ten million multiply-adds into about fifty thousand.

**Definition.** `pub struct PitchAnalysisFrame` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/pitch.rs`.

| Field | Type | Meaning |
|---|---|---|
| `energy` | `f64` | The frame energy `sum (s_LPF w_I)^2`. |
| `r_table` | `Vec<f64>` | `r(t)` (Eq. 7) for every integer lag `0..=R_LAGS` (`r(-t) = r(t)`). |
| `w4_sum` | `f64` | Sum of the fourth power of the window over the frame; the error function divides by `energy * (1 - P * w4_sum)` (Eq. 5). |

## `Complex` [@ANCHOR: ham_digital_modes:pitch_refinement_complex_struct]

**Purpose.** A complex number with two `f64` parts used by the pitch refinement DFT. Crate-private.

**Definition.** `pub(crate) struct Complex` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/pitch_refinement.rs`; derives `Clone`, `Copy`, `Debug`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `re` | `f64` | Real part. |
| `im` | `f64` | Imaginary part. |

**Used by.** `at`, `harmonic_amplitude`, `sw_at` (functions whose signature names it).

## `RefinementFrame` [@ANCHOR: ham_digital_modes:tia_102_baba_pitch_refinement_refinement_frame_struct]

**Purpose.** A single analysis frame's own `S_w(m)` (Eq. 29): the 256-point DFT of `s(n)*w_R(n)`, for `m` in `-127..=128` (precomputed once per frame, the same "expensive input, compute once" reasoning `pitch::PitchAnalysisFrame` already uses for `s_LPF`).

**Definition.** `pub struct RefinementFrame` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/pitch_refinement.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sw` | `[Complex; 256]` | The 256 complex bins `S_w(m)`, `m = -127..=128` (index 0 is bin -127). |

**Used by.** `harmonic_amplitude`, `new`, `refine_pitch`, `refinement_error` (functions whose signature names it).

## `SynthesisState` [@ANCHOR: ham_digital_modes:tia_102_baba_synthesis_synthesis_state_struct]

**Purpose.** Persistent decoder-side synthesis state: the shared noise generator (see `unvoiced_synthesis::UnvoicedState`'s own doc comment for why unvoiced and voiced synthesis must share one [`NoiseState`] rather than each owning an independent one), both synthesis halves' own state, and the two enhancement-stage state variables synthesis itself must carry (`S_E`, `tau_M`) -- `S_E = 75000.0` per Annex A; `tau_M` isn't itself listed in Annex A, but `20480.0` is Eq. 115's own first-branch value, which any real first frame (zero accumulated FEC errors, the only case that actually matters before a previous `tau_M` exists) reduces to regardless of the seed -- a reasoned default, not a guess.

**Definition.** `pub struct SynthesisState` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `noise` | `NoiseState` | The shared noise generator (`NoiseState`). |
| `unvoiced` | `UnvoicedState` | Unvoiced synthesis state. |
| `voiced` | `VoicedState` | Voiced synthesis state. |
| `s_e` | `f64` | Local energy `S_E` (initially 75000.0, Annex A). |
| `tau_m` | `f64` | Amplitude threshold `tau_M` (initially 20480.0, Eq. 115's first branch). |
| `first_frame` | `bool` | True until the first frame has been synthesized. |
| `last_final_amplitudes` | `Option<(f64, Vec<bool>, Vec<f64>)>` | This frame's own final synthesis inputs (post-enhancement, post-V/UV-forcing, post-gamma_M), kept around specifically for [`Self::synthesize_repeated_frame`]'s own Eq. 104 (`M_bar_l(0) = M_bar_l(-1)`) and the accompanying "reuse everything" reading of Eq. 99-104 -- `None` until the first real (non-repeated) frame has run. |
| `comfort_noise_seed` | `i64` | Section 7.8's own comfort-noise generator state: an independent instance of the *same* Eq. 117 recurrence [`NoiseState`] uses (same Annex A seed, `u(-105) = 3147` -- the spec defines exactly one such recurrence, and 7.8 doesn't name a second one, so reusing it is the spec-faithful reading, not an invented generator), but deliberately NOT sharing `noise`'s own state: a muted frame's own 160 raw comfort-noise draws would otherwise advance `NoiseState`'s shared window an amount unrelated to its own per-frame contract (`advance_frame`'s single step), desynchronizing the noise sequence real unvoiced/voiced synthesis depends on for every frame *after* the muted one. |
| `unvoiced_gain` | `f64` | Multiplier on the unvoiced half before the Eq. 142 sum: 1.0 for the standard; the D-STAR and AMBE+2 synthesizers set the value measured against the chip. |

**Notes.** Also holds `last_final_amplitudes` (for repeats), `comfort_noise_seed` (an independent noise generator for mutes) and `unvoiced_gain` (1.0 for the standard).

## `Complex` [@ANCHOR: ham_digital_modes:unvoiced_synthesis_complex_struct]

**Purpose.** A complex number with two `f64` parts used by the unvoiced-synthesis FFT (fast Fourier transform). Private to `unvoiced_synthesis.rs`.

**Definition.** `struct Complex` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/unvoiced_synthesis.rs`; derives `Clone`, `Copy`.

| Field | Type | Meaning |
|---|---|---|
| `re` | `f64` | Real part. |
| `im` | `f64` | Imaginary part. |

**Used by.** `unvoiced_dft`, `unvoiced_spectrum`, `unvoiced_time_domain` (functions whose signature names it).

## `NoiseState` [@ANCHOR: ham_digital_modes:tia_102_baba_unvoiced_synthesis_noise_state_struct]

**Purpose.** Persistent state for the noise generator `u(n)` (Eq. 117), seeded at `u(-105) = 3147`: the real state a decoder must carry frame to frame, since the recurrence can only be advanced one integer step at a time (it has no closed form in `n`, unlike [`synthesis_window`]). Keeps a rolling window of the most recent 209 values -- `u(n)` for `n` in the current frame's own `-104..=104` -- built by [`Self::new`] for the first frame and shifted forward by [`Self::advance_frame`] (160 new samples, "shifted by 20 ms.") for every frame after that.

**Definition.** `pub struct NoiseState` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/unvoiced_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `last` | `i64` | The most recent noise value of the recurrence. |
| `window` | `VecDeque<i64>` | Rolling window of the 209 most recent values (`u(n)` for `n` in `-104..=104`). |

**Used by.** `synthesize`, `unvoiced_dft`, `unvoiced_spectrum` (functions whose signature names it).

## `UnvoicedState` [@ANCHOR: ham_digital_modes:tia_102_baba_unvoiced_synthesis_unvoiced_state_struct]

**Purpose.** Persistent unvoiced-synthesis state: just the previous frame's own time-domain unvoiced signal (`u~_w(n, -1)`), needed by [`Self::synthesize`]'s own Eq. 126 overlap-add. The noise generator itself ([`NoiseState`]) is owned by the caller, not by this struct: Eq. 141's own `rho_l(0)` (used by voiced synthesis, `super::voiced_synthesis`) reads `u(l)` from this *same* current-frame noise window ("the shifted noise sequence for the current frame, described in Section 11.2" -- the spec's own words), not an independent one, so a single [`NoiseState`] must be shared between both halves of synthesis rather than each owning its own.

**Definition.** `pub struct UnvoicedState` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/unvoiced_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `previous_time_domain` | `[f64; 256]` | The previous frame's time-domain unvoiced signal (256 samples) for the overlap-add of Eq. 126. |

## `VoicedState` [@ANCHOR: ham_digital_modes:tia_102_baba_voiced_synthesis_voiced_state_struct]

**Purpose.** Persistent voiced-synthesis state: the running phase accumulator `psi_l(-1)` and the actual synthesis phase `phi_l(-1)` for every harmonic `1..=56`, plus the previous frame's own fundamental frequency, V/UV decisions, and enhanced spectral amplitudes -- everything Eq. 127-140 need from "the previous frame" that isn't already implicit in the current frame's own inputs. Initial values per Annex A (page 80): `omega0(-1) = .02985*pi`, `v_bar_l(-1) = false` (unvoiced) and `M_bar_l(-1) = 0.0` (silent) for all `l`, `phi_l(-1) = psi_l(-1) = 0.0` for all `l`, and `L~(-1) = 30` -- consistent, as a real cross-check rather than a coincidence, with this codebase's own encoder-side `FrameState::initial()` choice of a unity (not silent) starting spectral history: Annex A's own `M~_l(-1) = 1` (the *unenhanced* reconstructed amplitude, distinct from this struct's own `M_bar_l(-1)` the *enhanced* one) matches exactly.

**Definition.** `pub struct VoicedState` (struct) in `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/voiced_synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `psi` | `[f64; MAX_HARMONICS]` | Running phase accumulator `psi_l(-1)` per harmonic (array of `MAX_HARMONICS`). |
| `phi` | `[f64; MAX_HARMONICS]` | Synthesis phase `phi_l(-1)` per harmonic. |
| `omega0_prev` | `f64` | Previous frame's fundamental (initially `.02985*pi`). |
| `l_hat_prev` | `u32` | Previous frame's harmonic count (initially 30). |
| `voiced_prev` | `[bool; MAX_HARMONICS]` | Previous frame's voiced flag per harmonic (initially all false). |
| `amplitudes_prev` | `[f64; MAX_HARMONICS]` | Previous frame's enhanced amplitudes per harmonic (initially 0). |
| `amplitude_interp_shape` | `fn(f64) -> f64` | Shape of Eq. 135's `a_l(n)` amplitude interpolation, as a function of `t = n/N` in `[0,1]` (`shape(0) = 0`, `shape(1) = 1` is the caller's responsibility -- not enforced here since enforcing it would cost a per-sample check for no real benefit). Identity (`|t| t`) for the standard, matching Eq. 135 exactly; D-STAR and AMBE+2 may set a different shape via [`Self::set_amplitude_interpolation_shape`] -- see `super::super::mbe_synthesis`'s own doc comment for why (measured against the chip, not a TIA-102.BABA change). |

**Notes.** Also holds `amplitude_interp_shape`, a function pointer for Eq. 135's interpolation (identity for the standard; D-STAR and AMBE+2 may change it with `set_amplitude_interpolation_shape`).

