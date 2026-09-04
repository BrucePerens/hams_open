# Codec2 fixed-point codegen check

Standalone extraction of `lpc.rs`'s two `i128`-using functions (`q_mul`,
`div_round_i128`), used to answer a real question from
`../CODEC2_FIXED_POINT_WIDTH_REDUCTION_STUDY.md`: on the actual no-FPU target
class this port cares about (cheap HTs, ESP32-class parts), does the `i128`
intermediate in either function compile to an efficient inlined instruction
sequence, or a full software bignum-library call? This is a static codegen
inspection (cross-compile + disassemble), not an on-device cycle measurement
-- no real ESP32 hardware was used or is available in this environment.

## Recipe (Xtensa / real ESP32 LX6 core)

```
# One-time toolchain install (~2GB download, both an Xtensa-aware rustc
# fork and xtensa-esp-elf gcc/binutils -- rustc's own upstream LLVM does
# not support Xtensa):
cargo install espup --locked
espup install --targets esp32
source ~/export-esp.sh   # must be re-run in every new shell

cd codec2_fixedpoint_codegen_check
mkdir -p .cargo && cat > .cargo/config.toml <<'EOF'
[unstable]
build-std = ["core"]
EOF
cargo +esp build --release --target xtensa-esp32-none-elf -Z build-std=core

OBJDUMP=$(find ~/.rustup/toolchains/esp -name xtensa-esp32-elf-objdump | head -1)
$OBJDUMP -dr target/xtensa-esp32-none-elf/release/libcodec2_fixedpoint_codegen_check.a
```

`-dr` (not just `-d`) is required to resolve what `callx8` call sites
actually target -- the disassembly alone shows `l32r a8, <offset>` loading a
pointer from the function's own literal pool, not the symbol name; the `-r`
relocations against that `.literal.<fn>` section name the real target.

## Recipe (ARM Cortex-M4F, `thumbv7em-none-eabihf`)

Same idea, using stock `rustup` (no Xtensa fork needed) and the toolchain's
own `llvm-objdump`/`llvm-nm` (found under
`~/.rustup/toolchains/<nightly-or-beta>/lib/rustlib/<host-triple>/bin/`,
since the host `objdump` on an x86_64 dev box cannot disassemble ARM/Xtensa
object code):

```
rustup target add thumbv7em-none-eabihf
cargo build --release --target thumbv7em-none-eabihf
```

## What this resolved (real, on-target result)

See the study doc's own "Real, measured findings" section for the full
writeup. Summary: `q_mul`'s `i128` product is fully inlined on both targets
using native 32x32->64 widening multiply instructions (Xtensa `mull`/
`muluh`, ARM `umull`/`umlal`) -- no call, no software bignum-multiply
routine to eliminate. `div_round_i128`'s division genuinely calls out to
`__divti3` (a real, generic 128-bit signed division routine from
compiler-builtins) -- confirmed via `-dr` relocations, distinguishing it
from the function's other `callx8` sites, which resolve to the cold
`panic_const_div_by_zero` path, not the divide itself.
