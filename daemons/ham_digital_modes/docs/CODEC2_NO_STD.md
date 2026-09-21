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

Real crate, `riscv32imc`, fat link-time optimization, opt-level 3, 200 frames of real speech,
per-stage marks off (`--no-default-features`). The output checksums are identical to before the
rate specialisation: `f59f8fcd` for the 8 kHz run (also the host build's) and `9f8929d7` for the 16 kHz
(`decode_16k_fixed`) run, which the `decode16k` bench feature added. Steady average instructions per
20 ms frame, measured 2026-09-20:

| | Before | After |
| --- | --- | --- |
| Encode, 8 kHz | 644,149 | 478,024 |
| Decode, 8 kHz, `codec2_16k_bridge` not compiled in | 1,317,832 | 712,234 |
| Decode, 8 kHz, bridge compiled in (`--features bridge`, 16 kHz path not called) | 1,344,266 (1,345,916 with per-stage marks on) | 712,478 |
| Decode, 16 kHz (`decode_16k_fixed`, `--features decode16k`) | 2,498,432 | 1,211,040 |
| Worst frame (encode / decode 8 kHz / decode 16 kHz) | 649,867 / 1,360,876 / 2,602,565 | 483,341 / 770,915 / 1,333,775 |
| State (struct size), encoder / decoder | 11,424 / 9,592 (28,544 with the bridge) | unchanged |
| Stack, call tree below the caller, encode / decode | 7,820 / 11,156 | 8,096 / 11,140 (10,984 with the bridge) |

(With the `decode16k` harness, whose `main` inlines both decode entry points, the 8 kHz decode reads 721,558
against 1,344,970 before; that difference is in how the harness is compiled, not in the codec.)

These count instructions, not cycles. Compiling the bridge in no longer costs the 8 kHz decoder anything
(before: 2.1 percent, from the run-time size dispatch in the transform).

### What changed (all bit-exact)

The codec runs at exactly two sample rates, so everything that depends only on the rate is a compile-time
constant of a monomorphised copy:

* `fixed_fft.rs` is generic over the transform size through `Size<N>` (`Rate8k` = 512, `Rate16k` = 1024)
  and an `FftTables` trait carrying the per-size twiddle and bit-reversal tables. There is no run-time
  size `match` and no slice-length generics. Twiddles are `i32` pairs and the bit-reversal table `u16`, both
  built at compile time from the checked-in tables.
* The inverse transform of the harmonic spectrum (`SparseInverse`) scatters the harmonics straight to their
  bit-reversed positions, keeps a bitmap of which blocks of each stage can be nonzero and never touches or
  clears the rest, and in the last stage computes only the outputs the overlap-add reads (real parts at 160 of
  512 / 320 of 1024 points). Its 64-bit multiply kernel uses unsigned first-quadrant twiddles (exactly
  rotating the data for the upper quadrant), so it needs no sign corrections; the switch to the checked
  128-bit path moved from a value sum of 2^37 to 2^39, the actual bound.
* The forward transform of the 11 linear-prediction coefficients is an instance specialised for exactly that input count: no
  clearing, no permutation, and the runs of equal values that the first stages produce are written directly
  instead of being copied up stage by stage. The pitch estimator's 64-input transform uses the same machinery
  with its own twiddle table (which differs from the decoder's in the last bit at 124 entries, so it is kept).
* The pitch estimator's 512-point transform is pruned to the output bins it reads. The estimator uses only
  bins 0 to 128 (`PITCH_FFT_NEEDED_BINS`). In a radix-2 decimation-in-time transform the last stage combines two
  256-point results, so it now computes only the lower output of each butterfly with index 128 or below and
  skips the other 127 butterflies (each a full complex multiply); the stage before it computes only the
  lower outputs, plus the upper output of butterfly 0 (bin 128 of the second half-size result). Every bin that is
  computed uses exactly the same arithmetic as before, so the estimator's results are bit-identical
  (checksum unchanged); the entries of the output arrays above bin 128 are unspecified. This saved 29,530
  encode instructions per frame (507,554 to 478,024) and made the image about 0.7 KB smaller. The
  fallback for inputs beyond the 64-bit kernel's sum limit stays dense.
* The rate-independent tail of a sub-frame (`overlap_add_subframe<N, NS, SF>`) is shared by both rates; the
  harmonic bin scale (`k_q23`, a 64-bit division) is computed once per sub-frame in `ModelFixed`.
* `log2_q23` / `exp2_q23` run in 32-bit arithmetic with a table leading-zero count (these cores have no
  count-leading-zeros instruction), swept bit for bit against the previous 64-bit forms by a test; the
  linear-prediction spectrum magnitude and the pitch decimation filter use 32x32 products.

Tests added, none removed or weakened: the sparse inverse against the textbook `i128` transform (random and
structured harmonic layouts, empty, overwritten and one-sided spectra, exactly at the kernel limit, both
sizes); the constant-input-count forward transform; `log2_q23`/`exp2_q23` against their 64-bit reference. The
dense-transform tests now run through const-generic helpers at both sizes (same inputs and reference).

### Flash footprint and the code-size versus speed trade

Text plus read-only data, without the bench's 64,000 bytes of speech (the code figure still includes the bench's own
harness, about 8 KB, the same in both columns):

| | Before | After |
| --- | --- | --- |
| 8 kHz image, code | 65,044 | 84,096 |
| 8 kHz image, read-only data | 84,456 | 79,704 |
| 8 kHz image, total | 149,500 | 163,800 (+9.6 percent) |
| bridge and 16 kHz decode, code | 73,342 | 105,820 |
| bridge and 16 kHz decode, read-only data | 99,968 | 88,752 |
| bridge and 16 kHz decode, total | 173,310 | 194,570 (+12.3 percent) |

Tables (32-bit target): four 16,388-byte lookup tables (`TRIG_COS_Q23`, `TRIG_SIN_Q23`, `LPC_ACOS_LUT_Q23`,
`LPC_COS_LUT_Q23`, 65,552 bytes), the pitch estimator's transform twiddles (now 2,048 as `i32` pairs, was 4,096)
and Hann/low-pass (712), the analysis and synthesis windows (2,560), two 1,028-byte logarithm/exponential
tables, the 512-point transform twiddles (2,048, was 4,096) and bit-reversal table (1,024, was 2,048), the
256-byte leading-zero table, and small constants. `codec2_16k_bridge` adds about 8.7 KB (was 14.8 KB): the
1024-point twiddles (4,096) and bit-reversal table (2,048) and the 2,560-byte overlap window.

Code grows by 19 KB (8 kHz) because the transforms are unrolled per stage and per size (largest pieces: sparse
inverse 8.9 KB at 512 points and 9.1 KB at 1024, pitch transform 7.1 KB, forward linear-prediction transform 3.4 KB, the
compact any-mode fallbacks about 2.7 KB each) while the tables shrink by 4.7 KB (8 kHz) and 11.2 KB (bridge
image). Total growth is under the 20 percent warning level. Choice made: every stage of the hot instances is
its own unrolled copy. Sharing one run-time-length loop for the stages up to length 16 saves 3 KB but costs
5 percent more decode instructions (752,433 against 717,184 in the same build), so it was not kept. The cold
modes (inputs too large for the fast kernels, the test-only dense transform) use compact loops and are
bit-identical.

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

## Two kernel sets: 32-bit cores and 64-bit hosts

Several of the speedups above are tuned for a core that has no count-leading-zeros instruction
and no 64-bit multiplier (riscv32imc, ESP32 class). On a 64-bit host they are slower than the
plain form, and after the rate specialisation and pitch pruning work the host encoder took about
60 percent more instructions per frame than before it (measured 2026-09-20). The cause was one
function: the pitch estimator's 512-point transform (`pitch_prefix_i64_hot`), whose products were
split into 32-bit halves. On a 64-bit core that is roughly three times the instructions of one
multiply.

Each affected kernel now exists in two forms, and every target compiles both:

* **Split form** (32-bit halves, table-based leading-zero count): `MODE_I64` in
  `src/codec2_3200/fixed_fft.rs` and `log2_q23_split32` in `src/codec2_3200/fixed_point.rs`.
* **Native form** (plain 64-bit products, hardware `leading_zeros`): `MODE_I64_NATIVE` and
  `log2_q23_native`.

The choice is one constant, `NATIVE_64_BIT = cfg!(target_pointer_width = "64")`. Targets with
64-bit pointers (x86-64, aarch64) get the native form. Every 32-bit target (riscv32imc,
thumbv7m, 32-bit ARM) keeps the split form, so the QEMU counts are unchanged (478,024 encode and
712,234 decode instructions per frame, checksum f59f8fcd). The condition is the width of the
multiplier, not the presence of a leading-zero instruction: Cortex-M3 has one, but its 64-bit
products still cost several instructions, and it is not a measured target.

Unit tests run both forms on the same inputs and assert identical integers
(`butterfly_kernels_agree_bit_for_bit`, `pitch_transform_matches_the_dense_reference_in_both_64_bit_kernels`,
the sparse inverse trials in `fixed_fft.rs`, and the sweep in
`log2_and_exp2_q23_match_their_64_bit_reference_bit_for_bit`), so neither form can rot.

Host measurements (x86-64, Intel i7-1360P, `examples/codec2_fixed_bench`, checksum
`9ba18e2006300778` in all cases). Instruction counts come from `valgrind --tool=callgrind`
(`perf` is not installed here) and are the primary figure; they do not depend on machine load.
Wall-clock times are the minimum over 100 passes taken while the machine was quiet, but the box
is shared and its load average was around 30 for most of the session, so wall-clock numbers moved
by a factor of two or more between runs and only the ratios are meaningful.

| Build | Encode instr/frame | Decode instr/frame | Encode us | Decode us |
| --- | --- | --- | --- | --- |
| before the rate specialisation (242d598a) | 213,000 | 673,000 | (not taken quiet) | (not taken quiet) |
| origin/main before this change (b22e9b3b) | 300,000 | 460,000 | 18.1 | 29.2 |
| with the native kernels | 188,000 | 356,000 | 12.6 | 25.7 |

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
