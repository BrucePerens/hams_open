#!/bin/bash
# Guards the `no_std` build of the fixed-point Codec2 3200 path (see docs/CODEC2_NO_STD.md).
#
#   tools/check_codec2_no_std.sh              # everything below
#   tools/check_codec2_no_std.sh --lib-only   # only the library builds (no bench link, no QEMU)
#
# 1. Builds the library with `--no-default-features` (with and without the 16 kHz bridge) for the
#    bare-metal targets riscv32imc-unknown-none-elf and thumbv7m-none-eabi, warnings denied. Any
#    reintroduced `std`, `alloc`, `rustfft`, or float-only method (`.sqrt()`, `.sin()`) fails here.
# 2. Links the bench harness (tools/codec2_riscv32_bench) and fails if the binary contains a
#    software floating-point routine (`__addsf3`, `__muldf3`, `__floatsisf`, ...) or a math library
#    function (`sinf`, `sqrtf`, `powf`, ...). Runs it under QEMU when available and compares the
#    checksum with the host build's (`f59f8fcd`).
# Missing rustup targets are reported and skipped with exit status 0 only when SKIP_MISSING=1.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$HERE/..
LIB_ONLY=0
[ "${1:-}" = "--lib-only" ] && LIB_ONLY=1
export CARGO_TARGET_DIR=${NO_STD_TARGET_DIR:-${CARGO_TARGET_DIR:-$CRATE/target}/no_std_check}
# Cargo prefers CARGO_ENCODED_RUSTFLAGS over RUSTFLAGS, so under `cargo llvm-cov` (which sets it to
# `-C instrument-coverage`, needing a profiler runtime bare-metal targets lack) the RUSTFLAGS below
# would be silently ignored and the build would fail. Scrub every coverage/wrapper variable so this
# check always builds the plain, uninstrumented no_std library, whatever runs it.
unset CARGO_ENCODED_RUSTFLAGS RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER LLVM_PROFILE_FILE CARGO_LLVM_COV \
  CARGO_LLVM_COV_TARGET_DIR CARGO_LLVM_COV_SHOW_ENV RUSTDOCFLAGS CARGO_ENCODED_RUSTDOCFLAGS
export RUSTFLAGS="-D warnings"
EXPECTED_CHECKSUM=f59f8fcd
EXPECTED_CHECKSUM_1600=db5b738b   # Codec2 1600 fixed encoder + decoder, same excerpt, 100 frames of 40 ms

installed=$(rustup target list --installed 2>/dev/null || true)
for t in riscv32imc-unknown-none-elf thumbv7m-none-eabi; do
  if ! grep -qx "$t" <<<"$installed"; then
    echo "missing rustup target $t (rustup target add $t)"
    [ "${SKIP_MISSING:-0}" = 1 ] && exit 0
    exit 1
  fi
  for feat in "" "codec2_16k_bridge"; do
    echo "== build --no-default-features ${feat:+--features $feat }for $t"
    (cd "$CRATE" && cargo build --release --lib --no-default-features ${feat:+--features $feat} --target "$t")
  done
done
[ "$LIB_ONLY" = 1 ] && { echo "library builds ok"; exit 0; }

echo "== bench harness link + symbol check"
BENCH=$CRATE/tools/codec2_riscv32_bench
"$BENCH/prep.sh"
(cd "$BENCH" && RUSTFLAGS="-D warnings -C link-arg=-Tlink.ld" cargo build --release --no-default-features \
    --target-dir "$CARGO_TARGET_DIR/bench" --target riscv32imc-unknown-none-elf)
ELF=$CARGO_TARGET_DIR/bench/riscv32imc-unknown-none-elf/release/codec2_riscv32_bench
NM=$(command -v llvm-nm || command -v rust-nm || true)
[ -n "$NM" ] || { echo "llvm-nm not found (apt install llvm, or rustup component add llvm-tools)"; exit 1; }
BAD=$("$NM" "$ELF" | awk '{print $NF}' | grep -E '^(__[a-z]+[sd]f[0-9]|__(float|fix)[a-z]*[sd]?i|__(extend|trunc)[a-z]*f[a-z]*[0-9]|__(add|sub|mul|div|neg|cmp|eq|ne|lt|le|gt|ge|unord)[sd]f[0-9]|(sin|cos|tan|acos|asin|atan2?|sqrt|pow|exp2?|log2?|log10|floor|ceil|round|trunc|fabs|fmod)f?)$' || true)
if [ -n "$BAD" ]; then
  echo "FAIL: floating-point / math library symbols in the linked binary:"; echo "$BAD"; exit 1
fi
echo "no soft-float or math library symbols in $(basename "$ELF")"
if command -v qemu-system-riscv32 >/dev/null; then
  OUT=$(timeout 300 qemu-system-riscv32 -M virt -m 64M -nographic -bios none -icount shift=0 -kernel "$ELF")
  echo "$OUT" | grep -E "sizeof|first-frame|peak"
  echo "$OUT" | grep -q "checksum $EXPECTED_CHECKSUM" || { echo "FAIL: checksum differs from $EXPECTED_CHECKSUM"; exit 1; }
  echo "checksum $EXPECTED_CHECKSUM matches the host build"
else
  echo "qemu-system-riscv32 not installed: link check only"
fi

# Codec2 1600 (fixed encoder and decoder, also core-only): its own harness build, same two checks.
(cd "$BENCH" && RUSTFLAGS="-D warnings -C link-arg=-Tlink.ld" cargo build --release --no-default-features \
    --features mode1600 --target-dir "$CARGO_TARGET_DIR/bench1600" --target riscv32imc-unknown-none-elf)
ELF1600=$CARGO_TARGET_DIR/bench1600/riscv32imc-unknown-none-elf/release/codec2_riscv32_bench
BAD=$("$NM" "$ELF1600" | awk '{print $NF}' | grep -E '^(__[a-z]+[sd]f[0-9]|__(float|fix)[a-z]*[sd]?i|__(extend|trunc)[a-z]*f[a-z]*[0-9]|__(add|sub|mul|div|neg|cmp|eq|ne|lt|le|gt|ge|unord)[sd]f[0-9]|(sin|cos|tan|acos|asin|atan2?|sqrt|pow|exp2?|log2?|log10|floor|ceil|round|trunc|fabs|fmod)f?)$' || true)
if [ -n "$BAD" ]; then
  echo "FAIL: floating-point / math library symbols in the Codec2 1600 binary:"; echo "$BAD"; exit 1
fi
echo "no soft-float or math library symbols in the Codec2 1600 build"
if command -v qemu-system-riscv32 >/dev/null; then
  OUT=$(timeout 300 qemu-system-riscv32 -M virt -m 64M -nographic -bios none -icount shift=0 -kernel "$ELF1600")
  echo "$OUT" | grep -E "MODE1600"
  echo "$OUT" | grep -q "checksum1600 $EXPECTED_CHECKSUM_1600" || { echo "FAIL: 1600 checksum differs from $EXPECTED_CHECKSUM_1600"; exit 1; }
  echo "checksum1600 $EXPECTED_CHECKSUM_1600 matches the host build"
fi
echo "OK"
