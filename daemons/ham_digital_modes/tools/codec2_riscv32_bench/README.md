# codec2_riscv32_bench

Bare-metal instruction-count harness for the fixed-point Codec2 3200 path on a 32-bit RISC-V core with no floating point (riscv32imc, the ESP32-C3 class), run under QEMU. `prep.sh` has the build and run recipe. The firmware is tested by running it under QEMU from `../../tests/codec2_riscv32_bench_qemu.rs` (needs `qemu-system-riscv32` and `rustup target add riscv32imc-unknown-none-elf`). The function entries are in `../../docs/stories/codec2_riscv32_bench.md`.
