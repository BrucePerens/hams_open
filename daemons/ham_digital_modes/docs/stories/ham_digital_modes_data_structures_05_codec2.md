# Codec2 data structures (codec2_1600 and codec2_3200)

As a developer changing this code, I want every data structure in `src/codec2_1600/bits.rs`, `src/codec2_1600/lsp_quantiser.rs`, `src/codec2_1600/mod.rs`, `src/codec2_3200/bits.rs`, `src/codec2_3200/encoder_fixed.rs`, `src/codec2_3200/envelope.rs`, `src/codec2_3200/floating_reference/mod.rs`, `src/codec2_3200/floating_reference/nlp.rs`, `src/codec2_3200/floating_reference/voicing.rs`, `src/codec2_3200/lpc.rs`, `src/codec2_3200/quantise.rs`, `src/codec2_3200/spectral_bridge.rs`, `src/codec2_3200/synthesis.rs`, `src/codec2_3200/tables_gen.rs` and `src/codec2_3200/voicing.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `FrameFields1600` [@ANCHOR: ham_digital_modes:bits_frame_fields1600_struct]

**Purpose.** One 1600 bit/s (M17) frame's field values: 64 bits per 40 ms, as the reference's `codec2_encode_1600` orders them: two voicing bits, pitch and energy of the first 20 ms half, two voicing bits, pitch and energy of the second half, then ten LSP indices (36 bits in all).

**Definition.** `pub struct FrameFields1600` (struct) in `daemons/ham_digital_modes/src/codec2_1600/bits.rs`.

| Field | Type | Meaning |
|---|---|---|
| `voiced0` | `bool` | Voicing of sub-frame 0 (1 bit). |
| `voiced1` | `bool` | Voicing of sub-frame 1 (1 bit). |
| `wo_index_a` | `u32` | Pitch (`Wo`) index of the first half (`WO_BITS` bits). |
| `e_index_a` | `u32` | Energy index of the first half (`E_BITS` bits). |
| `voiced2` | `bool` | Voicing of sub-frame 2 (1 bit). |
| `voiced3` | `bool` | Voicing of sub-frame 3 (1 bit). |
| `wo_index_b` | `u32` | Pitch index of the second half (`WO_BITS` bits). |
| `e_index_b` | `u32` | Energy index of the second half (`E_BITS` bits). |
| `lsp_indexes` | `[u32; LPC_ORD]` | The ten LSP scalar quantizer indices, `lsp_bits(i)` bits each. |

**Used by.** `pack_frame_1600` (functions whose signature names it).

**Notes.** Written and read MSB-first, each field Gray-coded, by `pack_frame_1600` and `unpack_frame_1600` in `bits.rs` through `BitWriter` and `BitReader`.

## `LspDim` [@ANCHOR: ham_digital_modes:lsp_quantiser_lsp_dim_struct]

**Purpose.** One dimension's scalar codebook: evenly spaced from `start` in steps of `step`, `levels` entries (`2^log2m == levels` in every real dimension here, confirmed against each `lspN.txt`'s own `1 <m>` header). Every one of the ten real tables happens to be evenly spaced, so this is `start + step*index`, not a lookup array.

**Definition.** `struct LspDim` (struct) in `daemons/ham_digital_modes/src/codec2_1600/lsp_quantiser.rs`.

| Field | Type | Meaning |
|---|---|---|
| `start_hz` | `i64` | Frequency of level 0, in Hz. |
| `step_hz` | `i64` | Spacing between levels, in Hz. |
| `levels` | `u32` | Number of levels (`2^log2m`). |
| `log2m` | `u32` | Bits of the index. |

**Used by.** `quantise_dim` (functions whose signature names it).

**Notes.** Private table row of the 1600 bit/s LSP scalar quantizer: the value of index `i` is `start_hz + step_hz * i` (all ten real dimensions are evenly spaced).

## `Encoder` [@ANCHOR: ham_digital_modes:codec2_1600_encoder_struct]

**Purpose.** Persistent per-call encoder state -- same shape as `codec2_3200::floating_reference::Encoder`'s own, since the underlying analysis pipeline (pitch/voicing/LPC) is identical; this mode just runs it across four 10ms steps instead of two, and quantizes/transmits LSPs from only the last of them.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/codec2_1600/mod.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sn` | `[f32; M_PITCH]` | The `M_PITCH`-sample speech history window. |
| `analysis_window` | `[f32; M_PITCH]` | The analysis window applied to the history for LPC analysis. |
| `nlp_state` | `fnlp::NlpState` | The pitch estimator's persistent state. |
| `voicing_state` | `fvoicing::VoicingState` | The voicing decision's persistent state. |

**Notes.** Needs the `std` feature. One per stream; it runs the 3200 analysis pipeline across four 10 ms steps and quantizes LSPs from the last of them.

## `Decoder` [@ANCHOR: ham_digital_modes:codec2_1600_decoder_struct]

**Purpose.** Persistent per-decoder state: the previous frame's own decoded `Wo`/voiced/LSPs/energy (sub-frame 3's own, needed to interpolate the next frame's sub-frames 0/1/2), plus `synthesis::SynthesisState`'s own overlap-add memory and a forward FFT (fast Fourier transform) plan. `spectral_bridge` is the same opt-in 16kHz harmonic-extension state `codec2_3200::Decoder` carries -- see `decode_16k` below and `codec2_3200::spectral_bridge`'s own doc comment; `.enabled` toggles it (on by default), the ordinary `decode()` above never touches it.

**Definition.** `pub struct Decoder` (struct) in `daemons/ham_digital_modes/src/codec2_1600/mod.rs`.

| Field | Type | Meaning |
|---|---|---|
| `prev_wo` | `f32` | Previous frame's decoded pitch (sub-frame 3), used to interpolate the next frame's sub-frames. |
| `prev_voiced` | `bool` | Previous frame's voicing. |
| `prev_lsps` | `[f32; LPC_ORD]` | Previous frame's LSPs. |
| `prev_e` | `f32` | Previous frame's energy. |
| `synth` | `synthesis::SynthesisState` | The overlap-add synthesis state (`SynthesisState`). |
| `spectral_bridge` | `codec2_3200::spectral_bridge::SpectralBridgeState` | Opt-in 16 kHz harmonic-extension state (`enabled` toggles it; on by default; only `decode_16k` uses it). |

**Notes.** Needs the `std` feature. One per stream; `decode` mutates it.

## `BitWriter` [@ANCHOR: ham_digital_modes:bits_bit_writer_struct]

**Purpose.** Packs fields MSB-first into a byte buffer, Gray-coding any field wider than 1 bit before packing.

**Definition.** `pub struct BitWriter<'a>` (struct) in `daemons/ham_digital_modes/src/codec2_3200/bits.rs`.

| Field | Type | Meaning |
|---|---|---|
| `bits` | `&'a mut [u8]` | The output byte buffer being filled. |
| `bit_index` | `usize` | Position of the next bit to write (MSB first). |

**Notes.** Borrows the buffer mutably; fields wider than 1 bit are Gray-coded before packing.

## `BitReader` [@ANCHOR: ham_digital_modes:bits_bit_reader_struct]

**Purpose.** Reads fields MSB-first from a byte buffer, un-Gray-coding any field wider than 1 bit after unpacking.

**Definition.** `pub struct BitReader<'a>` (struct) in `daemons/ham_digital_modes/src/codec2_3200/bits.rs`.

| Field | Type | Meaning |
|---|---|---|
| `bits` | `&'a [u8]` | The input byte buffer. |
| `bit_index` | `usize` | Position of the next bit to read (MSB first). |

**Notes.** Borrows the buffer; fields wider than 1 bit are un-Gray-coded after unpacking.

## `FrameFields` [@ANCHOR: ham_digital_modes:bits_frame_fields_struct]

**Purpose.** One 3200bps frame's worth of decoded field values, before/after quantizer dequantization.

**Definition.** `pub struct FrameFields` (struct) in `daemons/ham_digital_modes/src/codec2_3200/bits.rs`.

| Field | Type | Meaning |
|---|---|---|
| `voiced0` | `bool` | Voicing of the first 10 ms sub-frame (1 bit). |
| `voiced1` | `bool` | Voicing of the second 10 ms sub-frame (1 bit). |
| `wo_index` | `u32` | Pitch (`Wo`) quantizer index (`WO_BITS` bits). |
| `e_index` | `u32` | Energy quantizer index (`E_BITS` bits). |
| `lsp_indexes` | `[u32; LPC_ORD]` | The ten LSP quantizer indices. |

**Used by.** `pack_frame` (functions whose signature names it).

**Notes.** Produced by the encoder's quantizers or by unpacking a frame; consumed by dequantization or packing. Plain data.

## `EncoderFixed` [@ANCHOR: ham_digital_modes:encoder_fixed_encoder_fixed_struct]

**Purpose.** The integer-only 3200 bit/s encoder state: raw `i16` sample history plus the pitch and voicing state. No `f32` or `f64` is touched at run time; every table is a committed constant in `tables.rs`.

**Definition.** `pub struct EncoderFixed` (struct) in `daemons/ham_digital_modes/src/codec2_3200/encoder_fixed.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sn` | `[i16; M_PITCH]` | Raw sample history, `i16`-native. The whole encode path is integer: pitch, energy and line spectral pairs reach their quantizers as integers (pitch bin, Q23 energy, Q23 radians); no `f32` or `f64` is touched at run time, and every table is a committed constant in `tables.rs`. |
| `nlp_state` | `nlp::NlpStateFixed` | The fixed-point pitch estimator state (`NlpStateFixed`). |
| `voicing_state` | `voicing::VoicingStateFixed` | The fixed-point voicing state (`VoicingStateFixed`). |

**Notes.** Fields `sn` and `nlp_state` are `pub(super)`; one per stream.

## `Model` [@ANCHOR: ham_digital_modes:envelope_model_struct]

**Purpose.** Sinusoidal-synthesis model parameters for one 10ms sub-frame: pitch (`wo`, normalized angular frequency), harmonic count (`l`), per harmonic amplitude/phase (`a`/`phi`, both 1-indexed -- index 0 unused, matching the harmonics' own 1-based numbering), and voicing.  `phi` stores each harmonic's phase as a unit complex vector (`cos`/`sin` already evaluated) rather than an angle in radians. `synthesize_phase` derives it from `h[m] * ex` by dividing out the magnitude directly -- no `atan2` needed, since the only later use (`synthesize_subframe`'s IFFT-input construction) immediately turns an angle back into exactly this same unit vector via `sin_cos`. That round trip (`atan2` then `sin_cos`) was an identity for every harmonic postfilter didn't overwrite; storing the vector directly removes both the `atan2` call and one of the two `sin_cos` calls per harmonic, which matters once this runs in fixed point (no arctangent primitive needed at all, and the remaining `sin_cos` wants a single well-designed LUT rather than two).

**Definition.** `pub struct Model` (struct) in `daemons/ham_digital_modes/src/codec2_3200/envelope.rs`.

| Field | Type | Meaning |
|---|---|---|
| `wo` | `f32` | Fundamental frequency, normalised angular frequency (radians per sample). |
| `l` | `usize` | Number of harmonics. |
| `a` | `[f32; MAX_AMP + 1]` | Harmonic amplitudes, 1-indexed (index 0 unused), `MAX_AMP + 1` entries. |
| `phi` | `[Complex32; MAX_AMP + 1]` | Harmonic phases as unit complex vectors (cos and sin already evaluated), 1-indexed. |
| `voiced` | `bool` | Whether the sub-frame is voiced. |

**Used by.** `apply_first_harmonic_correction`, `sample_filter_phase` (functions whose signature names it).

## `ModelFixed` [@ANCHOR: ham_digital_modes:envelope_model_fixed_struct]

**Purpose.** Fixed-point sibling of `Model` -- `wo`/`a` (Q23, `lpc::COEF_FRAC_ BITS`'s own format for `wo`; a linear amplitude for `a`, same fractional-bit count but real headroom needs -- see `interp.rs`'s own comment on why "23 fractional bits" doesn't imply one shared Q-format name across domains) and `phi` (Q23 unit complex vectors, `ComplexQ23`) genuinely integer, no `f32` anywhere.

**Definition.** `pub(crate) struct ModelFixed` (struct) in `daemons/ham_digital_modes/src/codec2_3200/envelope.rs`.

| Field | Type | Meaning |
|---|---|---|
| `wo` | `i64` | Fundamental in Q23. |
| `k_q23` | `i64` | `synth_k_q23(wo)`, the harmonic-to-bin scale, computed once here: it costs a 64-bit division and every stage of a sub-frame needs it. |
| `l` | `usize` | Number of harmonics. |
| `a` | `[i64; MAX_AMP + 1]` | Linear harmonic amplitudes (1-indexed), integer. |
| `phi` | `[ComplexQ23; MAX_AMP + 1]` | Phases as Q23 unit complex vectors (`ComplexQ23`), 1-indexed. |
| `voiced` | `bool` | Whether the sub-frame is voiced. |

**Used by.** `apply_first_harmonic_correction_fixed`, `sample_filter_phase_fixed`, `sample_filter_phase_with` (functions whose signature names it).

**Notes.** Crate-private; the `k_q23` field (harmonic-to-bin scale `synth_k_q23(wo)`, computed once per sub-frame) is documented in the source.

## `Encoder` [@ANCHOR: ham_digital_modes:floating_reference_encoder_struct]

**Purpose.** Persistent per-call encoder state: the `M_PITCH`-sample speech history window shared by pitch estimation and LPC analysis, plus `nlp`/`voicing`'s own state.

**Definition.** `pub struct Encoder` (struct) in `daemons/ham_digital_modes/src/codec2_3200/floating_reference/mod.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sn` | `[f32; M_PITCH]` | The `M_PITCH`-sample speech history window shared by pitch estimation and LPC analysis. |
| `window` | `[f32; M_PITCH]` | The analysis window. |
| `nlp_state` | `nlp::NlpState` | The pitch estimator's persistent state. |
| `voicing_state` | `voicing::VoicingState` | The voicing decision's persistent state. |

**Notes.** The float reference encoder; one per stream.

## `NlpState` [@ANCHOR: ham_digital_modes:nlp_nlp_state_struct]

**Purpose.** Persistent per-encoder pitch-estimator state.

**Definition.** `pub struct NlpState` (struct) in `daemons/ham_digital_modes/src/codec2_3200/floating_reference/nlp.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sq` | `[f32; M_PITCH]` | Squared, DC-notched signal history, one entry per original-rate sample across the `M_PITCH`-sample analysis window. |
| `mem_x` | `f32` | DC-notch filter memory (previous input, previous output). |
| `mem_y` | `f32` | DC-notch filter memory: the previous output. |
| `prev_f0` | `f32` | Previous frame's estimated fundamental (Hz), used to bias sub-multiple correction toward pitch continuity. |
| `fft` | `std::sync::Arc<dyn rustfft::Fft<f32>>` | Planned once and reused every call -- `wspr_sync.rs`'s own established convention in this same crate: `FftPlanner` and `plan_fft_forward` are real, non-trivial work that shouldn't repeat every 10ms frame. |
| `fft_buf` | `[Complex32; PE_FFT_SIZE]` | Reused scratch buffer for the FFT -- a fixed-size stack array (`PE_FFT_SIZE` is a compile-time constant), not a `Vec`, since this runs on a real-time codec's per-10ms-sub-frame encode path. |

**Used by.** `nlp` (functions whose signature names it).

## `VoicingState` [@ANCHOR: ham_digital_modes:voicing_voicing_state_struct]

**Purpose.** Persistent per-encoder voicing-decision state.

**Definition.** `pub struct VoicingState` (struct) in `daemons/ham_digital_modes/src/codec2_3200/floating_reference/voicing.rs`.

| Field | Type | Meaning |
|---|---|---|
| `noise_floor_db` | `f32` | Adaptively tracked background noise level, dB (updated only on frames judged unvoiced, the same "don't track during real speech" idea `synthesis.rs`'s own `bg_est` postfilter constants are named after, independently applied here on the encoder side). |

**Used by.** `is_voiced` (functions whose signature names it).

## `Autocorr` [@ANCHOR: ham_digital_modes:lpc_autocorr_struct]

**Purpose.** Autocorrelation of a speech frame: `LPC_ORD + 1` `f32` values (lags 0 to `LPC_ORD`), the input of `levinson_durbin_fixed`. Needs the `std` feature.

**Definition.** `pub type Autocorr = [f32; LPC_ORD + 1];` (type) in `daemons/ham_digital_modes/src/codec2_3200/lpc.rs`.

**Used by.** `fixed_clamp_fired_per_iteration`, `float_clamp_fired_per_iteration`, `levinson_durbin_fixed`, `levinson_durbin_fixed_core`, `levinson_durbin_fixed_point_normalized`, `quantize_r` (functions whose signature names it).

## `LpcCoeffs` [@ANCHOR: ham_digital_modes:lpc_lpc_coeffs_struct]

**Purpose.** Linear-prediction coefficients: `LPC_ORD + 1` `f32` values (index 0 is the leading 1.0), the output of `levinson_durbin_fixed` (a leading 1.0 at index 0 is the usual convention; unknown whether every caller relies on it). Needs the `std` feature.

**Definition.** `pub type LpcCoeffs = [f32; LPC_ORD + 1];` (type) in `daemons/ham_digital_modes/src/codec2_3200/lpc.rs`.

**Used by.** `levinson_durbin_fixed`, `levinson_durbin_fixed_point_normalized` (functions whose signature names it).

## `LpcCoeffsQ` [@ANCHOR: ham_digital_modes:lpc_lpc_coeffs_q_struct]

**Purpose.** LPC coefficients in Q8.23 fixed-point (`COEF_FRAC_BITS`), the real internal representation `levinson_durbin_fixed_core` computes in -- `levinson_durbin_fixed` converts this to `LpcCoeffs` (`f32`) only at its own output boundary, matching this port's established "integer core, float boundary" pattern.

**Definition.** `type LpcCoeffsQ = [i64; LPC_ORD + 1];` (type) in `daemons/ham_digital_modes/src/codec2_3200/lpc.rs`.

**Used by.** `levinson_durbin_fixed_core` (functions whose signature names it).

## `LspDim` [@ANCHOR: ham_digital_modes:quantise_lsp_dim_struct]

**Purpose.** One dimension of the LSP delta-scalar quantizer: 32 levels (5 bits), `step1`Hz apart for the first `breakpoint` levels, `step2`Hz apart after that (`step1 == step2` for a purely uniform dimension).

**Definition.** `pub(crate) struct LspDim` (struct) in `daemons/ham_digital_modes/src/codec2_3200/quantise.rs`.

| Field | Type | Meaning |
|---|---|---|
| `step1` | `f32` | Spacing in Hz of the first `breakpoint` levels. |
| `breakpoint` | `u32` | Number of levels with the first spacing. |
| `step2` | `f32` | Spacing in Hz of the remaining levels. |

**Used by.** `lsp_dim_value_hz` (functions whose signature names it).

## `LspDimQ16` [@ANCHOR: ham_digital_modes:quantise_lsp_dim_q16_struct]

**Purpose.** Q16-fixed-point sibling of `LspDim`: `step1`/`step2`/breakpoint are always exact integer Hz values in the real reference (`25.0`, `50.0`), so unlike `HZ_PER_RAD` (a genuine irrational ratio, still a one-time float conversion below), these need no float conversion at all -- `25i64 << LSP_HZ_FRAC_BITS` is exact.

**Definition.** `struct LspDimQ16` (struct) in `daemons/ham_digital_modes/src/codec2_3200/quantise.rs`.

| Field | Type | Meaning |
|---|---|---|
| `step1_q16` | `i64` | Step of the first segment, Hz, Q16 (exact: 25 Hz). |
| `breakpoint` | `u32` | Number of levels in the first segment. |
| `step2_q16` | `i64` | Step of the second segment, Hz, Q16 (exact: 50 Hz). |

**Used by.** `lsp_dim_nearest_level_q16`, `lsp_dim_value_hz_q16` (functions whose signature names it).

## `SpectralBridgeState` [@ANCHOR: ham_digital_modes:spectral_bridge_spectral_bridge_state_struct]

**Purpose.** Persistent per-decoder Spectral Bridge synthesis state -- parallel to, and independent of, `synthesis::SynthesisState` (the base 8kHz path is untouched by this module entirely). `ex_phase` is updated with the *exact* formula `synthesize_phase` uses (`wo * N_SAMP`, the *original* `N_SAMP`, not `N_SAMP_SB` -- phase is a physical, sample-rate-independent quantity, and both paths advance it once per 10ms sub-frame from the same `Wo`), so harmonics `1..=l`'s own already-decoded phase and this module's own extrapolated harmonics' phase stay consistent with each other from a shared, if independently tracked, phase origin.

**Definition.** `pub struct SpectralBridgeState` (struct) in `daemons/ham_digital_modes/src/codec2_3200/spectral_bridge.rs`.

| Field | Type | Meaning |
|---|---|---|
| `enabled` | `bool` | Switch for the harmonic extension (public). |
| `sn_` | `[f32; SAMPLES_PER_FRAME_SB]` | Overlap-add buffer of the 16 kHz path. |
| `parzen` | `[f32; SAMPLES_PER_FRAME_SB]` | The Parzen window used for the overlap-add. |
| `ex_phase` | `f32` | Excitation phase of the extrapolated harmonics, advanced like the base synthesis (`wo * N_SAMP`). |
| `rng` | `u32` | Phase source for the unvoiced high band (`synthesize_subframe_sb_from_envelope`); its own state, never shared with the base synthesis, so the 8 kHz output is unchanged. |
| `ifft_buf` | `[Complex32; FFT_ENC_SB]` | `microfft::inverse::ifft_1024` (no stored FFT plan needed) is normalized (divides by N); `synthesize_subframe_sb` below multiplies its result by `FFT_ENC_SB` to preserve this module's existing unnormalized-IFFT convention -- see `synthesis.rs`'s own identical pattern and `Cargo.toml`'s comment on the `microfft` dependency for the verification this relies on. |

**Notes.** Independent of `SynthesisState`: the 8 kHz path is untouched by it. Also keeps the unvoiced-band random generator `rng` and the inverse-FFT buffer `ifft_buf`.

## `SpectralBridgeStateFixed` [@ANCHOR: ham_digital_modes:spectral_bridge_spectral_bridge_state_fixed_struct]

**Purpose.** Fixed-point sibling of `SpectralBridgeState` -- genuinely integer end to end (no `f32` until the final `i16` PCM conversion, same "integer core, float boundary" pattern `DecoderFixed` establishes elsewhere in this crate). No `rng`/unvoiced excitation-phase branch here at all, matching `synthesize_subframe_sb`'s own (post-review) shape -- extrapolation only ever runs voiced.

**Definition.** `pub(crate) struct SpectralBridgeStateFixed` (struct) in `daemons/ham_digital_modes/src/codec2_3200/spectral_bridge.rs`.

| Field | Type | Meaning |
|---|---|---|
| `enabled` | `bool` | Switch for the harmonic extension. |
| `sn_` | `[i64; SAMPLES_PER_FRAME_SB]` | Integer overlap-add buffer of the 16 kHz path. |
| `ex_phase_q32` | `u32` | Excitation phase as a `u32` turn fraction (Q32). |
| `rng` | `u32` | Phase source for the unvoiced high band (`synthesize_subframe_sb_from_envelope_fixed`), independent of the base synthesis's generator. |
| `ifft_re` | `[i64; FFT_ENC_SB]` | Real part of the inverse-FFT scratch. |
| `ifft_im` | `[i64; FFT_ENC_SB]` | Imaginary part of the inverse-FFT scratch. |

**Notes.** Crate-private fixed-point sibling; extrapolation only ever runs voiced.

## `SynthesisState` [@ANCHOR: ham_digital_modes:codec2_3200_synthesis_synthesis_state_struct]

**Purpose.** Persistent per-decoder synthesis state.

**Definition.** `pub struct SynthesisState` (struct) in `daemons/ham_digital_modes/src/codec2_3200/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sn_` | `[f32; SAMPLES_PER_FRAME]` | `SAMPLES_PER_FRAME`-sample overlap-add buffer: the newest `N_SAMP` samples are finished output, the rest carries the windowed tail of the previous frame's own IDFT waiting to be summed into the next one. |
| `parzen` | `[f32; SAMPLES_PER_FRAME]` | The Parzen window for the overlap-add. |
| `ex_phase` | `f32` | Excitation phase, advanced per sub-frame. |
| `bg_est` | `f32` | Background noise estimate used by the postfilter. |
| `rng` | `u32` | Random generator state for unvoiced phases. |
| `ifft_buf` | `[Complex32; FFT_ENC]` | `FFT_ENC` is a compile-time constant, and this buffer is reused every call -- a fixed-size stack array, not a `Vec`, since this runs on a real-time codec's per-10ms-sub-frame decode path. `microfft::inverse::ifft_512` (no stored FFT plan needed, unlike `rustfft`'s trait-object planner) is normalized (divides by N); `synthesize_subframe` below multiplies its result by `FFT_ENC` to preserve this module's existing unnormalized-IFFT convention -- verified directly against `rustfft::plan_fft_inverse` before this swap (see `Cargo.toml`'s own comment on the `microfft` dependency). |

**Notes.** Needs the `std` feature. Also holds the overlap-add buffer `sn_` and the inverse-FFT buffer `ifft_buf` (documented in the source).

## `SynthesisStateFixed` [@ANCHOR: ham_digital_modes:synthesis_synthesis_state_fixed_struct]

**Purpose.** Fixed-point sibling of `SynthesisState` -- genuinely integer end to end. `ifft` is `fixed_fft::fft_fixed` (`forward: false`, matching `rustfft`'s own unnormalized inverse convention, verified against it directly in `fixed_fft.rs`'s own tests) rather than an `Arc<dyn Fft<f32>>`, so there's no trait object or planner here at all.

**Definition.** `pub(crate) struct SynthesisStateFixed` (struct) in `daemons/ham_digital_modes/src/codec2_3200/synthesis.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sn_` | `[i64; SAMPLES_PER_FRAME]` | Integer overlap-add buffer (`SAMPLES_PER_FRAME`). |
| `ex_phase` | `u32` | Excitation phase as a `u32` turn fraction. |
| `bg_est` | `i64` | Background noise estimate (integer). |
| `rng` | `u32` | Random generator state for unvoiced phases. |
| `scratch` | `FftScratch` | Transform buffers, also lent to the envelope stage (see `FftScratch`). |

**Notes.** Crate-private fixed-point sibling; `scratch` (`FftScratch`) holds the transform buffers, also lent to the envelope stage.

## `Value` [@ANCHOR: ham_digital_modes:tables_gen_value_enum]

**Purpose.** One entry of the test-only table generator that writes the committed `tables.rs`: a scalar constant, an array of integers, or an array of integer pairs, each with a name and doc text. Test-only (`tables_gen.rs`); the drift test regenerates every value and fails if the committed file differs.

**Definition.** `pub(super) enum Value` (enum) in `daemons/ham_digital_modes/src/codec2_3200/tables_gen.rs`.

| Variant | Type | Meaning |
|---|---|---|
| `Scalar` | `(&'static str, &'static str, i64)` | A named integer constant: (name, doc text, value). |
| `Array` | `(&'static str, &'static str, &'static str, Vec<i64>)` | A named integer array: (name, element type, doc text, values). |
| `Pairs` | `(&'static str, &'static str, Vec<(i64, i64)>)` | A named array of integer pairs: (name, doc text, pairs). |

**Used by.** `all_values` (functions whose signature names it).

## `VoicingStateFixed` [@ANCHOR: ham_digital_modes:voicing_voicing_state_fixed_struct]

**Purpose.** Fixed-point candidate for `floating_reference::voicing::is_voiced` (`FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s voicing.rs stage). `is_voiced`'s own `energy_db = 10*log10(energy)` step turned out to be a real dependency on `fixed_point.rs`'s `log2_lut` (whose own interpolation is still genuine `f32`, a gap flagged separately in this punch list) -- rather than build on top of that incomplete piece, this sidesteps the log entirely: the dB-domain comparison `energy_db > noise_floor_db + MARGIN_DB` is mathematically equivalent to a **linear**-domain comparison `energy > noise_floor_energy * 10^(MARGIN_DB/10)`, so the threshold check needs no runtime log or pow at all, just one precomputed constant ratio.  **This is a real, deliberate algorithmic difference, not a numerically-identical refactor**: tracking the noise floor as an EMA over *linear* energy is not the same as an EMA over *dB* (a log-domain EMA is closer to a geometric-mean-flavored average of the underlying energies; a linear-domain EMA is arithmetic-flavored) -- permissible per this module's own doc comment ("doesn't need to reproduce [the reference]'s exact decision, only make a reasonable one"), but validated below against the *same* real test scenarios `is_voiced` itself is validated against, not assumed equivalent.

**Definition.** `pub struct VoicingStateFixed` (struct) in `daemons/ham_digital_modes/src/codec2_3200/voicing.rs`; derives `Default`.

| Field | Type | Meaning |
|---|---|---|
| `noise_floor_energy` | `i64` | Linear mean-squared-amplitude units (raw `i16` sample scale), not dB. |

**Used by.** `is_voiced_fixed` (functions whose signature names it).

**Notes.** Default is a zero noise floor; the field is in linear mean-squared-amplitude units of the raw `i16` sample scale, not dB. The comparison it feeds is done in the linear domain, a deliberate difference from the float `is_voiced` that is validated against the same test scenarios.

