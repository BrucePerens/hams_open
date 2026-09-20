# Codec2 3200 without the standard library (`no_std`)

The fixed-point Codec2 3200 encoder (`EncoderFixed`) and decoder (`DecoderFixed`) build for 32-bit
microcontrollers that have no floating-point unit, no heap and no operating system. It uses only
Rust's `core` library. `alloc` is not needed and not used.

## Build commands

```
rustup target add riscv32imc-unknown-none-elf thumbv7m-none-eabi      # once
cargo build --release --lib --no-default-features --target riscv32imc-unknown-none-elf
cargo build --release --lib --no-default-features --target thumbv7m-none-eabi
# add --features codec2_16k_bridge for 16 kHz decoder output
tools/check_codec2_no_std.sh              # all of the above, plus link, symbol and QEMU checks
cargo test --release --test codec2_3200_no_std_build   # the library builds, run from the test suite
```

`riscv32imc-unknown-none-elf` is the ESP32-C3 core (32-bit RISC-V with integer multiply/divide and
compressed instructions, no floating point, no atomics). The default build (`cargo build`) is
unchanged: everything in the crate, with the standard library.

## Cargo features

| Feature | Default | Meaning |
| --- | --- | --- |
| `std` | on | Everything else in the crate: FT8, WSPR, PSK31, RTTY, D-STAR, the whole AMBE tree, Codec2 1600, the floating-point Codec2 3200 reference (`floating_reference`, `Decoder`, float adapters), and the dependencies they use (`libm`, `rustfft`, `microfft`). Also the vendored C library build in `build.rs`, which is skipped without `std`. |
| `codec2_16k_bridge` | on with `std` | The 16 kHz output "spectral bridge" (`DecoderFixed::decode_16k_fixed`, `spectral_bridge`), its tables and the 1024-point Fast Fourier Transform tables. |
| `codec2_profile` | off | Per-stage instruction-count hooks used by the bench harness. Expands to nothing when off. |
| `ambe_plus_2` | off | Unchanged (needs `std`). |

Without `std` the crate contains `bits`, `encoder_fixed`, `envelope`, `fixed_fft`, `fixed_point`,
`interp`, `lpc`, `nlp`, `quantise`, `synthesis`, `tables`, `trig_fixed`, `voicing`, `window` (the
integer parts of each) and the integer decoder. Codec2 1600 stayed behind `std`: it is built on the
floating-point line-spectral-pair code and was not worth converting.

Every item that contains floating point (there were 77 in the old inventory) is gated
`#[cfg(feature = "std")]`. `tests/codec2_3200_fixed_no_float_tokens.rs` enforces that no ungated
item outside `#[cfg(test)]` contains an `f32`/`f64` token; the bare-metal build catches float
method calls such as `.sqrt()` because `core` does not have them.

## Measured on the QEMU RISC-V bench (`tools/codec2_riscv32_bench`)

Real crate, `riscv32imc`, fat link-time optimization, opt-level 3, 200 frames of real speech.
The output checksum (`f59f8fcd`) is identical to the host build and to the earlier copied-source harness.

| | Encode | Decode |
| --- | --- | --- |
| Instructions per 20 ms frame (steady average) | 644,149 | 1,317,832 |
| Worst frame | 649,867 | 1,360,876 |
| State (struct size) | 11,424 bytes | 9,592 bytes (28,544 with `codec2_16k_bridge`) |
| Stack, call tree below the caller | 7,820 bytes | 11,156 bytes |

These count instructions, not cycles. The earlier harness (with inline-prevention edits) measured
644,534 and 1,317,366 instructions and 7,756 / 11,284 bytes of stack, so the numbers agree to
within about 0.1 percent. With the bridge feature on, decode measured 1,345,916 because inlining
choices changed; that has not been investigated.

Flash (read-only) footprint:

* Code: about 50 KB for encode, decode, the Fast Fourier Transform and pitch estimator (link-time
  optimized, includes 128-bit and 64-bit integer division helpers of about 3.3 KB from
  `compiler_builtins`; there are no floating-point or `libm` symbols).
* Tables (32-bit target, `i64` entries are 8 bytes): about 81 KB: four 16,388-byte lookup tables
  (`TRIG_COS_Q23`, `TRIG_SIN_Q23`, `LPC_ACOS_LUT_Q23`, `LPC_COS_LUT_Q23`, 65,552 bytes), the pitch
  estimator's Fast Fourier Transform twiddles (4,096) and Hann/low-pass (712), the analysis and
  synthesis windows (2,560), two 1,028-byte logarithm/exponential tables, the 512-point transform
  twiddles (4,096) and bit-reversal table (2,048), and small constants.
* `codec2_16k_bridge` adds about 14.8 KB: the 1024-point transform twiddles (8,192) and bit-reversal
  table (4,096) and the 2,560-byte overlap window.

Shrinking candidates (not implemented, because none is bit-exact):

* `TRIG_SIN_Q23` is `TRIG_COS_Q23` shifted by a quarter turn, but the committed tables were
  generated in `f32` and differ by one unit in the last place (1.2e-7) at 2,165 of 4,097 entries.
  Saves 16 KB of flash. Needs the tables regenerated from one quarter-wave (or `f64`) source,
  which changes decoder output and the golden hashes, so it is Bruce's decision.
* `LPC_COS_LUT_Q23` covers `[0, pi]` and is antisymmetric, so half would do (8 KB), with the same
  one-unit rounding difference at 1,992 entries.
* Halving a table and interpolating would raise the linear-interpolation error about four times;
  that accuracy was not measured.
* `LPC_ACOS_LUT_Q23` is steep near 1 and does not halve.

## Remaining work for a real ESP32 image

* Memory map and linker script, start-up code, and a `panic` handler for the real chip (the bench's
  `link.ld` and `_start` are QEMU-specific). Call `EncoderFixed::new()` / `DecoderFixed::new()` once;
  they need 11 KB and 10 KB (29 KB with the bridge) of static memory. Keep the stack at 12 KB or more
  for the calling task.
* The tables must sit in flash that the chip can read with acceptable latency (instruction/data
  cache); the four lookup tables are accessed at random and will miss the cache often. Cycle counts
  per frame have not been measured on real silicon; the instruction counts above are only the start.
* Hardware abstraction layer and Inter-IC Sound (I2S) audio driver for the microphone and speaker,
  8 kHz sample conversion, and the framing (160 samples in, 8 bytes out, 20 ms).
* Chip choice: ESP32-C3 (RISC-V `riscv32imc`) works with the stock Rust compiler. The C6 and H2 are
  `riscv32imac`. The classic ESP32, S2 and S3 are Xtensa, which needs Espressif's fork of the Rust
  compiler (installed with `espup`, target `xtensa-esp32-none-elf`); the crate has not been built
  or run on Xtensa.
