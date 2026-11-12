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

## Host tests

`cargo test` builds the crate with std (the `no_std` attribute and the
`panic_handler` apply only to the real cross-compiled build) and checks each
function's arithmetic on the host, with no target hardware.

### `q_mul_i128` [@ANCHOR: codegen_check_q_mul_i128]

A Q40 fixed-point multiply: the product of two numbers each scaled by 2^40, rescaled to Q40 with rounding half up. It is the copy of `lpc.rs`'s `q_mul`, the function whose 128-bit intermediate the study inspects. On both targets the product compiles to inline widening multiplies, with no library call.

| Argument | Type | Meaning |
|---|---|---|
| `a`, `b` | `i64` | Q40 operands (value times 2^40); any `i64`. |

**Returns:** `i64`, `(a * b + 2^39) >> 40`, computed in `i128` so full-range inputs cannot overflow the product. The final cast to `i64` truncates if the true result does not fit, which cannot happen for operands whose real values are within about 2^23 in magnitude. **Errors:** none; no panics (the shifts are constant). **Special concerns:** `#[no_mangle]` so it survives into the static library for disassembly; pure, no state. **See also:** `src/lib.rs` test `q_mul_is_a_rounded_q40_product`.

### `div_round_i128_fn` [@ANCHOR: codegen_check_div_round_i128_fn]

Divides a 128-bit numerator by a 128-bit denominator and rounds half away from zero; the copy of `lpc.rs`'s `div_round_i128`. This is the function that really calls out to the compiler-builtins routine `__divti3` on the no-FPU targets.

| Argument | Type | Meaning |
|---|---|---|
| `n` | `i128` | Numerator; its sign picks the rounding direction. |
| `d` | `i128` | Denominator; must be non-zero. |

**Returns:** `i64`, `(n + d/2) / d` for `n >= 0` and `(n - d/2) / d` otherwise, truncated to `i64`. **Errors:** a zero `d` panics (the `panic_const_div_by_zero` path that the disassembly shows as a cold branch); a negative `d` is not handled specially. **Special concerns:** pure; it is the slow part of the Levinson-Durbin recursion on a chip without hardware division. **See also:** test `div_round_rounds_half_away_from_zero`.

### `cheb_poly_eval_fixed_i64` [@ANCHOR: codegen_check_cheb_i64]

Evaluates a degree-5 Chebyshev series at a point using the Chebyshev recurrence, in Q29 fixed point with 64-bit storage; the real implementation in `lpc.rs`. It is kept here so its code generation can be compared with the 32-bit candidate.

| Argument | Type | Meaning |
|---|---|---|
| `coef` | `&[i32; 6]`, borrowed | The six Q29 series coefficients, `coef[5]` multiplies T0 and `coef[0]` multiplies T5. |
| `x_q` | `i64` | The evaluation point in Q29, nominally -1 to +1 (so -2^29 to 2^29). |

**Returns:** `i64`, the series value in Q29. **Errors:** none; intermediate values are `i64` and do not overflow in the range given. **Special concerns:** pure; five multiplies by `x_q` and six by coefficients. **See also:** test `cheb_i64_matches_the_float_recurrence`.

### `cheb_poly_eval_fixed_i32` [@ANCHOR: codegen_check_cheb_i32]

The same evaluation with 32-bit storage for `x_q`, the recurrence terms and the sum, widening to 64 bits only inside each multiply. It exists to test whether a narrower type is cheaper on a 32-bit core. It returns exactly the `i64` version's value wherever the narrower sum does not overflow.

| Argument | Type | Meaning |
|---|---|---|
| `coef` | `&[i32; 6]`, borrowed | Q29 coefficients, as above. |
| `x_q` | `i32` | Evaluation point in Q29, within -2^29 to 2^29. |

**Returns:** `i32`, the Q29 series value; **Errors:** none, but a series whose true value exceeds 2^31 in Q29 wraps (the casts truncate), which is why the result is only claimed equal where the storage does not overflow. **See also:** test `cheb_i32_is_bit_exact_with_i64_in_range`.

The bare `panic` handler (`loop {}`) is not tested: it never returns and exists only in the no_std build.
