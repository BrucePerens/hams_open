#!/bin/bash
# Instruction-count harness for the fixed-point Codec2 3200 path on a 32-bit RISC-V core with no
# floating point and no 64-bit multiply (riscv32imc, the ESP32-C3 class), run under QEMU.
# It depends on the REAL crate (`ham_digital_modes`, `--no-default-features`, so `no_std`); nothing
# is copied any more. See docs/CODEC2_NO_STD.md.
#
#   rustup target add riscv32imc-unknown-none-elf     # once; QEMU: qemu-system-riscv32
#   ./prep.sh                                          # extracts the speech excerpt the bench encodes
#   cargo build --release                              # (add --features small for a 6-frame build,
#                                                      #  --features bridge to include the 16 kHz bridge,
#                                                      #  --features decode16k to also time decode_16k_fixed (own checksum),
#                                                      #  --no-default-features for no per-stage marks)
#   qemu-system-riscv32 -M virt -m 64M -nographic -bios none -icount shift=0 \
#       -kernel target/riscv32imc-unknown-none-elf/release/codec2_riscv32_bench
#
# The program reads the `minstret` counter (instructions retired) around each frame and prints
# average instructions per 20 ms frame, plus per-stage splits from the crate's `profile_mark!` hooks.
# This counts INSTRUCTIONS, not cycles: cycles per instruction on a real core depend on multiplier
# latency, load/store wait states and flash cache.
# For an instruction-class mix (ALU / multiply / load / store / branch), so that cycles can be
# estimated from an assumed per-class latency table, run the 6-8 frame `small` build under Unicorn:
#   cargo build --release --features small
#   llvm-objcopy -O binary target/riscv32imc-unknown-none-elf/release/codec2_riscv32_bench bench.bin
#   python3 -m venv v && v/bin/pip install unicorn
#   v/bin/python mixcount.py bench.bin target/riscv32imc-unknown-none-elf/release/codec2_riscv32_bench 8
# (the encode phase count also includes the harness's own text formatting, so use the class
# percentages, not the totals; the entry points are the out-of-line `do_encode`/`do_decode` wrappers in src/main.rs).
# Its checksum equals the host build's `cargo run --release --example codec2_fixed_bench --
# <wav> 1 100 200`, proving the 32-bit build is bit-identical.
#
# Float / library-call check (acceptance: none): see ../check_codec2_no_std.sh, which also builds this.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
FIX="$HERE/../../tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav"
# 200 frames of real speech starting 2 s in (frame 100), plus a 6-frame excerpt for the `small` build.
dd if="$FIX" of="$HERE/speech.raw" bs=1 skip=$((44+2*16000)) count=$((160*2*200)) 2>/dev/null
head -c $((320*8)) "$HERE/speech.raw" > "$HERE/speech_small.raw"
