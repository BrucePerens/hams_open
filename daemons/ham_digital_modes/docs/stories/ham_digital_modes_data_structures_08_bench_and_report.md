# Data structures of the codec report example and the RISC-V benchmark

As a developer changing this code, I want the data structures in `examples/ambe_roundtrip_quality_report.rs` and `tools/codec2_riscv32_bench/src/main.rs` explained in full (ADR 0113): what each is for, its fields, and how it is used. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `Codec` [@ANCHOR: ham_digital_modes:ambe_roundtrip_quality_report_codec_struct]

**Purpose.** One row of the codec table of `examples/ambe_roundtrip_quality_report.rs`: a display name and a function that encodes 8 kHz 16-bit PCM and decodes it again, so the report can measure each codec port the same way (energy correlation, spectral distance and level offset against the input).

**Definition.** `type Codec = (&'static str, fn(&[i16]) -> Vec<f64>);` (a private type alias for a tuple) in `daemons/ham_digital_modes/examples/ambe_roundtrip_quality_report.rs`.

| Field | Type | Meaning |
|---|---|---|
| `.0` | `&'static str` | Name printed in the report, for example `D-STAR float`. |
| `.1` | `fn(&[i16]) -> Vec<f64>` | Round trip: PCM (pulse-code modulation) in, decoded PCM out (a whole number of 160-sample frames). |

**Notes.** `codec_table()` builds the list: the D-STAR and TIA-102.BABA float and fixed ports, plus the AMBE+2 float and fixed ports when the `ambe_plus_2` feature is on. The functions are pure; there is no state between calls and no hardware. Test: `tests::every_codec_in_the_table_turns_pcm_into_finite_pcm_of_whole_frames` in the example.

## `Uart` [@ANCHOR: ham_digital_modes:main_uart_struct]

**Purpose.** The text output of the bare-metal RISC-V benchmark `tools/codec2_riscv32_bench`: a zero-sized type that implements `core::fmt::Write` by writing every byte to the memory-mapped UART at address `0x1000_0000` (the QEMU virt machine's UART), so `write!` and `println`-style output reaches the emulator's console. Defined in `daemons/ham_digital_modes/tools/codec2_riscv32_bench/src/main.rs` (a `no_std` program).

**Definition.** `struct Uart;` (unit struct, no fields).

**Behaviour and concerns.** `write_str` does one volatile byte write per character, never fails (always `Ok`), does not wait for the UART and does no buffering or locking; it assumes it is the only writer, as in the single-core benchmark. It touches a fixed physical address, so it is only meaningful on the emulated machine the benchmark is built for. Test: `tests/codec2_riscv32_bench_qemu.rs`, which runs the benchmark under QEMU and reads its output.
