# Data structures of the ham_digital_modes examples (generators and parameter records)

As a developer changing this code, I want every data structure in `examples/ambe_chip_silence_probe.rs`, `examples/ambe_error_concealment_eval.rs`, `examples/ambe_error_concealment_eval_tia.rs`, `examples/jmbe_frames.rs`, `examples/tia_102_baba_cross_validation_dump.rs`, `examples/tia_102_baba_oracle_bits_dump.rs`, `examples/tia_102_baba_oracle_quant_dump.rs`, `examples/ambe_plus_2_chip_encode_compare.rs`, `examples/dstar_chip_encode_compare.rs` and `examples/ratet27_chip_encode_compare.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `Lcg` [@ANCHOR: ham_digital_modes:ambe_chip_silence_probe_lcg_struct]

**Purpose.** A 64-bit linear congruential generator (multiplier 6364136223846793005, increment 1442695040888963407) used by the silence-probe example to make reproducible pseudo-random noise for the DVSI chip. Not cryptographic. Defined in `examples/ambe_chip_silence_probe.rs`; `next` returns a value in -1.0..1.0.

**Definition.** `struct Lcg(u64);` (struct) in `daemons/ham_digital_modes/examples/ambe_chip_silence_probe.rs`.

**Notes.** The state is the whole struct (one `u64`); the same seed always gives the same sequence. Test: the example's `tests` module.

## `Lcg` [@ANCHOR: ham_digital_modes:ambe_error_concealment_eval_lcg_struct]

**Purpose.** The same 64-bit LCG as in the other examples, used by `examples/ambe_error_concealment_eval.rs` to flip channel bits reproducibly in its error-rate sweeps; `unit` returns a value in 0.0..1.0 from the high 53 bits. Not cryptographic.

**Definition.** `struct Lcg(u64);` (struct) in `daemons/ham_digital_modes/examples/ambe_error_concealment_eval.rs`.

**Used by.** `corrupt` (functions whose signature names it).

**Notes.** One `u64` of state; the same seed gives the same error pattern. Test: the example's `tests` module.

## `Lcg` [@ANCHOR: ham_digital_modes:ambe_error_concealment_eval_tia_lcg_struct]

**Purpose.** The same 64-bit LCG, used by `examples/ambe_error_concealment_eval_tia.rs` for reproducible channel-error patterns in the TIA-102.BABA concealment evaluation; `unit` returns a value in 0.0..1.0.

**Definition.** `struct Lcg(u64);` (struct) in `daemons/ham_digital_modes/examples/ambe_error_concealment_eval_tia.rs`.

**Notes.** One `u64` of state; the same seed gives the same error pattern. Test: the example's `tests` module.

## `Lcg` [@ANCHOR: ham_digital_modes:jmbe_frames_lcg_struct]

**Purpose.** The same 64-bit LCG, used by `examples/jmbe_frames.rs` to inject reproducible bit errors (the `<ber>` and `<seed>` arguments) into the wire frames it emits for cross-checking with JMBE; `unit` returns a value in 0.0..1.0.

**Definition.** `struct Lcg(u64);` (struct) in `daemons/ham_digital_modes/examples/jmbe_frames.rs`.

**Used by.** `flip` (functions whose signature names it).

**Notes.** One `u64` of state. Test: the example's `tests` module.

## `Rng` [@ANCHOR: ham_digital_modes:tia_102_baba_cross_validation_dump_rng_struct]

**Purpose.** Deterministic xorshift for reproducible sweeps.

**Definition.** `struct Rng(u64);` (struct) in `daemons/ham_digital_modes/examples/tia_102_baba_cross_validation_dump.rs`.

**Notes.** One `u64` of state. Test: the example's `tests` module checks the first outputs for seed 1.

## `Rng` [@ANCHOR: ham_digital_modes:tia_102_baba_oracle_bits_dump_rng_struct]

**Purpose.** The same 64-bit xorshift generator, used by `examples/tia_102_baba_oracle_bits_dump.rs` to draw random frames for the oracle bit dump.

**Definition.** `struct Rng(u64);` (struct) in `daemons/ham_digital_modes/examples/tia_102_baba_oracle_bits_dump.rs`.

**Notes.** One `u64` of state; seeded in the example with `0x2545_f491_4f6c_dd1d`. Test: the example's `tests` module.

## `Rng` [@ANCHOR: ham_digital_modes:tia_102_baba_oracle_quant_dump_rng_struct]

**Purpose.** The same 64-bit xorshift generator, used by `examples/tia_102_baba_oracle_quant_dump.rs`, with `uniform` (0.0..1.0) and `normal` (roughly Gaussian, unit variance, the sum of 12 uniforms minus 6) helpers for random quantizer test data.

**Definition.** `struct Rng(u64);` (struct) in `daemons/ham_digital_modes/examples/tia_102_baba_oracle_quant_dump.rs`.

**Notes.** One `u64` of state. Test: the example's `tests` module.

## `P` [@ANCHOR: ham_digital_modes:ambe_plus_2_chip_encode_compare_p_struct]

**Purpose.** Raw parameter indices `b0` (pitch), `b1` and `b2` of one speech frame, extracted so the chip's frames and this crate's frames can be compared index by index. Defined in `examples/ambe_plus_2_chip_encode_compare.rs`; `params` fills it (`None` for a frame that is not speech).

**Definition.** `struct P` (struct) in `daemons/ham_digital_modes/examples/ambe_plus_2_chip_encode_compare.rs`.

| Field | Type | Meaning |
|---|---|---|
| `b0` | `u32` | Pitch code of the frame. |
| `b1` | `u32` | Voiced/unvoiced vector index. |
| `b2` | `u32` | Gain-delta index. |

**Used by.** `params` (functions whose signature names it).

## `P` [@ANCHOR: ham_digital_modes:dstar_chip_encode_compare_p_struct]

**Purpose.** Raw parameter indices `b0` (pitch), `b1` and `b2` of one D-STAR speech frame, extracted so the chip's frames and this crate's frames can be compared index by index. Defined in `examples/dstar_chip_encode_compare.rs`; `params` fills it (`None` for a tone frame).

**Definition.** `struct P` (struct) in `daemons/ham_digital_modes/examples/dstar_chip_encode_compare.rs`.

| Field | Type | Meaning |
|---|---|---|
| `b0` | `u32` | Pitch code of the frame. |
| `b1` | `u32` | Voiced/unvoiced vector index. |
| `b2` | `u32` | Gain-delta index. |

**Used by.** `params` (functions whose signature names it).

## `Params` [@ANCHOR: ham_digital_modes:ratet27_chip_encode_compare_params_struct]

**Purpose.** What `examples/ratet27_chip_encode_compare.rs` compares per frame between the DVSI chip's RATET(27) frames and this crate's encoder: pitch index, harmonic count, voiced fraction and gain index. `params_of` fills it (`None` for a frame that is a repeat or a mute).

**Definition.** `struct Params` (struct) in `daemons/ham_digital_modes/examples/ratet27_chip_encode_compare.rs`.

| Field | Type | Meaning |
|---|---|---|
| `b0` | `i32` | Pitch index (`i32`, the chip's log-scale code). |
| `l_hat` | `u32` | Number of harmonics. |
| `voiced_frac` | `f64` | Fraction of harmonics that are voiced (0.0 to 1.0). |
| `b2` | `u32` | Gain-delta index. |

