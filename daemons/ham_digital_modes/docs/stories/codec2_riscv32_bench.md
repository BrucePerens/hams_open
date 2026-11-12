# Story: the RISC-V firmware bench under QEMU (codec2_riscv32_bench)

Bare-metal instruction-count harness for the fixed-point Codec2 3200 path on a 32-bit RISC-V core with no floating point (riscv32imc, the ESP32-C3 class), run under QEMU. See `tools/codec2_riscv32_bench/prep.sh` for the build and run recipe. It is firmware: it has no operating system, so its functions cannot be called from an ordinary unit test. They are tested by running the real firmware under QEMU (an emulator, no hardware) from `tests/codec2_riscv32_bench_qemu.rs` in the parent crate, which builds the `small` variant, runs it, and checks the output and the checksum against the host build of the same codec.


## Functions (ADR 0113 entries)

### `Uart::write_str` [@ANCHOR: codec2_riscv32_bench_uart_write_str]

**Purpose.** The `core::fmt::Write` implementation that sends the report to the serial port: every byte of the string is written to the UART data register at address 0x1000_0000 (the QEMU `virt` machine's 16550) with a volatile write, so the writes are not merged or dropped.

**Arguments.** `s: &str`, borrowed; each byte is sent as is, with no translation of newlines.

**Returns.** Always `Ok(())`; the port has no flow control and no error to report.

**Special concerns.** Single core, no locking, not interrupt safe. Test: `the_riscv32_firmware_runs_under_qemu_reports_its_counters_and_matches_the_host_checksum` reads the text QEMU receives on the port.

### `exit` [@ANCHOR: codec2_riscv32_bench_exit]

**Purpose.** Ends the program and QEMU with a status. It writes to the test-finisher device at 0x10_0000 and then spins; exit code 0 is the word 0x5555 (QEMU exits 0), any other code is `(code << 16) | 0x3333` (QEMU exits with that code).

**Arguments.** `code: u32`, 0 for success.

**Returns.** Never. The panic handler calls it with 1 after printing the panic text.

**Special concerns.** On a real chip the address is not mapped, so this is for QEMU only. Test: the QEMU test asserts the emulator's exit status is 0.

### `paint_stack` [@ANCHOR: codec2_riscv32_bench_paint_stack]

**Purpose.** Prepares the stack high-water measurement: fills the unused stack area, from the end of the program's data (`__bss_end`) up to 256 bytes below the current stack pointer, with the pattern 0xA5A5A5A5.

**Arguments.** None.

**Returns.** The stack pointer at the call, which `stack_high_water` later takes as its baseline.

**Special concerns.** Writes raw memory below the stack pointer, so it must only run on this firmware's linker layout (`link.ld`); the 256-byte gap keeps its own frame safe. Test: the QEMU test requires a plausible nonzero stack figure for encode and decode, which is only possible when the painting worked.

### `stack_high_water` [@ANCHOR: codec2_riscv32_bench_stack_high_water]

**Purpose.** Reads the stack high-water mark back: walks up from `__bss_end` while the painted pattern is intact and returns how many bytes below the baseline the stack was used.

**Arguments.** `sp: usize`, the value `paint_stack` returned.

**Returns.** Bytes of stack used by the callees since the painting (not by the caller's own frame).

**Special concerns.** Reads raw memory; a program that legitimately writes the pattern value shortens the result. Test: the QEMU test checks the reported encode and decode stack figures are between 256 bytes and 60 kB.

### `main` [@ANCHOR: codec2_riscv32_bench_main]

**Purpose.** The firmware entry point (called from the assembly start-up code that sets the stack and clears the data area). It encodes the embedded speech excerpt frame by frame, then decodes every frame, counting retired instructions around each call and splitting them by stage from the codec's profile marks, tracks the stack high-water marks, prints the report on the UART and exits.

**Arguments.** None.

**Returns.** Never (`exit(0)`); an optional feature adds the 16 kHz bridge and Codec2 1600 measurements.

**Special concerns.** It prints a `checksum` over every encoded byte and every decoded sample; the host build of the same codec gives the same value, which proves the 32-bit build is bit-identical. Test: the QEMU test compares that checksum with the one the host computes, and checks the frame count, the instruction counts and the stage splits.

### `instret` [@ANCHOR: codec2_riscv32_bench_instret]

**Purpose.** Reads the RISC-V `minstret` counter: the number of instructions the core has retired. The harness subtracts two readings to count the instructions of a call, which is machine independent, unlike cycles.

**Arguments.** None.

**Returns.** The low 32 bits of the counter (wrapping subtraction handles rollover).

**Special concerns.** Only the machine-mode CSR read; it needs a core with that counter (QEMU `-icount shift=0` makes it exact). Test: the QEMU test requires encode and decode counts of hundreds of thousands of instructions per frame, with the average not above the maximum.

### `mark` [@ANCHOR: codec2_riscv32_bench_mark]

**Purpose.** The per-stage profile hook (the crate's `codec2_profile_mark` resolves to it): adds the instructions retired since the previous mark to bucket `i`; mark 15 only resets the baseline.

**Arguments.** `i: usize`, the bucket number (0 to 15) the codec passes for the stage that just ended.

**Returns.** Nothing.

**Special concerns.** Writes the global `ACC` table, so it is single threaded by design; the extra instructions of the hook itself are counted in the next stage. Test: the QEMU test requires the per-stage figures to be nonzero for the main stages and to add up to no more than the frame total.
