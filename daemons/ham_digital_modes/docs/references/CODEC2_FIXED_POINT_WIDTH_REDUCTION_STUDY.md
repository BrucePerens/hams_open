# Codec2 Fixed-Point Width-Reduction Study: Where Can 64/128-bit Narrow to 32?

## Status: Question 1 measured and closed (no win), 2026-09-04. Question 2 (Chebyshev
## Q2.29) and the divide-side optimization remain open. See "Real, measured findings"
## below -- this replaces the plan's own methodology item 4, which had recommended
## question 1 first as "highest-value, lowest-risk"; the value half of that claim is
## now measured false and is retracted.

Written per Bruce's own direct request: "do a study on codec2_3200 on where we can reduce the 64
bit and 128 bit fixed-point to 32 without too much loss of performance." Scopes a real measurement
study, not a guessed answer -- this codebase's own recent history on this exact code (the
Levinson-Durbin fixed-point build, same session) already demonstrated that narrowing a Q-format
without measuring against real, worst-case fixture data produces a silently-wrong result, not just
a slower-but-correct one.

## Ground truth, checked directly: where the 64/128-bit arithmetic actually is

A full grep of every `.rs` file in `codec2_3200/` for `i64`/`i128` found exactly one real
concentration: `lpc.rs`. Every other file (`fixed_point.rs`, `quantise.rs`, `synthesis.rs`,
`envelope.rs`, `interp.rs`, `nlp.rs`, `voicing.rs`, `window.rs`) is already `i32`-only for its real
DSP arithmetic. (`bits.rs`'s own `Vec<i64>` is a test/reference-file parser, not runtime DSP --
out of this study's scope entirely.) Two real algorithms, both in `lpc.rs`:

1. **Levinson-Durbin recursion** (`levinson_durbin_fixed_core`): a uniform internal Q8.40 format
   (`LEVINSON_FRAC_BITS = 40`, `i64` for `r_norm`/`e`/`k`/`a[]`), narrowed to Q8.23
   (`COEF_FRAC_BITS = 23`) only at the function's output boundary. `q_mul()` (the per-step multiply)
   and `div_round_i128()` (the per-step division computing `k`) both widen to `i128` for their real
   intermediate product/numerator before narrowing back to `i64`.
2. **Chebyshev polynomial evaluation for LSP root-finding** (`cheb_poly_eval_fixed`,
   `coarse_cheb_poly_eval_fixed`): Q2.29 (`CHEB_FRAC_BITS = 29`), `i64`-typed throughout, no `i128`
   step -- the per-term multiply (`coef_q[i] as i64 * t_prev`) stays within `i64`'s own range
   without needing a wider intermediate.

**Real, already-measured evidence directly bearing on this question, from this same session's own
prior work on Levinson-Durbin**: a narrower Q-format (Q8.23, matching `COEF_FRAC_BITS`, tried as the
recursion's own *internal* format, not just its output) was tested against this codebase's own real
fixture corpus and found insufficient -- at the worst-conditioned real frame in that corpus (frame
273, minimum `e` &approx; 2.76e-4), the recursion's own `k = -numerator/e` step amplifies whatever
quantization error already exists in `numerator` by roughly `1/e`, about 3600x at that frame. This
is not a hypothetical risk for this study -- it is the specific, measured reason Q8.40 (not
narrower) was chosen for Levinson-Durbin's own internal format in the first place. Any narrowing
study touching this function must treat that finding as a real constraint to test against, not
start from a blank slate.

## What "narrow to 32" actually means here, and the two real, different questions it splits into

1. **Eliminate `i128` (widen-then-narrow via a double-width intermediate), keep `i64`.** This is
   the narrower, more tractable question `q_mul()`/`div_round_i128()` alone raise -- does the
   *multiply/divide's own intermediate* genuinely need 128 bits, or can the same round-to-nearest
   result be computed with an `i64`-only technique (e.g. splitting a 64x64 multiply into
   high/low 32-bit halves, the standard technique real embedded fixed-point libraries use when the
   target has no native 128-bit or even 64-bit multiply)? This does NOT touch the outer Q8.40
   format's own already-measured precision requirement at all -- it's purely about how the
   intermediate arithmetic is computed, and is very plausibly narrowable without the precision loss
   the Q8.23-recursion experiment already found, since the *final* `i64` result is unchanged either
   way -- only how it's computed changes.
2. **Narrow the outer format itself (`i64` values down to `i32`).** This is the question the
   Levinson-Durbin recursion's own `LEVINSON_FRAC_BITS = 40` already has real, measured evidence
   against (see above) -- an `i32` can hold at most Q-format widths summing to 31 usable bits
   (excluding sign), and Q8.40's own 48-bit requirement (8 integer + 40 fractional) doesn't fit
   `i32` at all, let alone with headroom for the division-amplification finding's own real
   precision needs. This is the harder, higher-risk half of the study, and should not be assumed
   solvable just because question 1 might be.

## Real motivation for why this matters, on the actual target hardware

This port's own real goal (Bruce's own direction, same session): cheap HTs and ESP32-class parts
with no good FPU. Those same parts typically have real hardware support for a native 32x32->64
multiply (so `i64`-result arithmetic from `i32` inputs is often cheap), but no native 64x64 or
128-bit multiply/divide at all -- `i128` arithmetic on such a target is emulated via multiple
32-bit instructions per operation, a real, measurable CPU cost this codec runs continuously, not
occasionally. Eliminating `i128` (question 1) is likely to be the highest-value, lowest-risk part
of this study for exactly that reason -- it may recover most of the real performance benefit
without touching the part of the format that's already known to need real precision headroom.

## Real study methodology, matching this codebase's own established discipline

1. **Reuse the existing discriminator-test pattern** (`levinson_durbin_fixed_diverges_from_float_
   only_at_measured_clamp_disagreement_frames`, `lpc.rs`'s own test module) as the real acceptance
   check for any narrowed variant: does it diverge from the current, validated `i64`/`i128`
   implementation only at already-understood clamp-disagreement frames, or does it introduce new,
   unexplained divergence -- the same bar the original fixed-point build itself had to clear against
   the float reference.
2. **Measure against the full real fixture corpus, not a hand-picked sample** -- explicitly
   including frame 273 (the known worst-conditioned frame) and any other frame the existing test
   suite already flags as a real edge case, not just typical/average frames where a narrower format
   would look deceptively fine.
3. **A real CPU-cost measurement on the actual target class of hardware, not just x86 wall-clock**,
   matching this session's own `probe_sideband_and_offset_search_command_time_cost`/
   `probe_psk31_bank_scaling_cost` precedent for "measure before claiming a performance win" --
   ideally cross-compiled and run on a real ESP32 (or at minimum, a cycle-accurate estimate of
   32-bit-vs-64-bit-vs-128-bit multiply/divide cost on that architecture), not assumed from x86
   behavior, which has native 64-bit and reasonably fast 128-bit (via `MUL`/`IMUL` pairs) arithmetic
   that doesn't represent the real target's own cost structure at all.
4. **Question 1 (eliminate `i128`) first, independently of question 2** -- it's the smaller, more
   isolated, lower-risk change (touches `q_mul()`/`div_round_i128()`'s own internals, not the
   `LEVINSON_FRAC_BITS`/`COEF_FRAC_BITS` format constants anything else depends on), and real
   evidence from it (does an `i64`-only intermediate reproduce the exact same rounded result as the
   `i128` version, bit-for-bit, across the whole fixture corpus) can be gathered before deciding
   whether question 2 is even worth attempting.
5. **Chebyshev/LSP's own Q2.29 (`cheb_poly_eval_fixed`) is a real, separate, second candidate**,
   not yet measured at all this session -- its own per-term multiply already stays within `i64`
   without an `i128` step, so question 1 doesn't apply there; whether its own `i64` VALUES (not just
   intermediates) could narrow to `i32` is a genuinely open, unmeasured question, worth its own real
   fixture-corpus test the same way Levinson-Durbin's was, not assumed to behave the same way just
   because it's structurally similar.

## Real open questions for Bruce, not resolved by this plan

1. **Real ESP32 access for Part 3's own hardware measurement** -- does this session/environment
   have real target hardware to cross-compile and measure against, or does the study need to
   proceed on cycle-accurate architectural estimates only (a real, named limitation to disclose
   honestly if so, not silently treated as equivalent to a real measurement)?
2. ~~Priority between question 1 and question 2~~ -- **resolved by measurement, not by Bruce's
   choice**: question 1 (eliminate `i128` in `q_mul()`) is a measured no-op on both real target ISAs
   checked (see "Real, measured findings" above) and is not being built. The real, separate
   opportunity this measurement surfaced -- replacing `div_round_i128()`'s `__divti3` call with a
   reciprocal-multiply technique -- is a new, higher-risk question of its own (bit-exactness is not
   achievable; the acceptance bar is "diverges only at already-measured clamp-disagreement frames,"
   unverified without a real fixture-corpus run) and is not yet started. Question 2 (Chebyshev/LSP's
   Q2.29 narrowing) remains open and independent of both.

## Real, measured findings (2026-09-04): Question 1 is a no-op, don't build it

Per Bruce's direction to proceed ("Do the codec2 reduction... this change would be welcome"),
`q_mul()` and `div_round_i128()` were extracted byte-for-byte into a standalone `no_std` crate
(`codec2_fixedpoint_codegen_check/`, next to this document) and cross-compiled to real target ISAs
to inspect what the `i128` arithmetic actually lowers to -- this answers "does the intermediate
genuinely need a software bignum routine, or does the compiler already emit an efficient inlined
sequence" directly, without guessing from x86 behavior. See that crate's own README.md for the
exact recipe. **This is codegen inspection on the real target instruction set, not an on-device
cycle measurement** -- no real ESP32/Cortex-M4 hardware was used or is available in this
environment; that gap (open question 1, below) is unchanged.

Two targets checked: `thumbv7em-none-eabihf` (ARM Cortex-M4F class, via stock `rustup`) and, since
the actual target chip this port cares about is ESP32 specifically, `xtensa-esp32-none-elf` (the
real Xtensa LX6 core, via the `espup`-installed Xtensa Rust fork -- upstream `rustc`'s own LLVM does
not support Xtensa at all).

**`q_mul()`'s `i128` product: fully inlined on both targets, zero calls.** On Xtensa LX6: 0xcc bytes
(~74 instructions), built entirely from the chip's own native 32x32->64 widening multiply
instructions (`mull`/`muluh`) plus carry-propagation adds/compares -- confirmed via `objdump -dr`
that there is no `callx8` anywhere in the function body. On ARM Cortex-M4F: 0x6a bytes (~33
instructions), same shape, built from `umull`/`umlal`. **This resolves methodology item 4's own
premise as false: there is no software 128-bit multiply routine here to eliminate.** LLVM already
does, on both real target ISAs, exactly what a hand-written split-multiply replacement would try to
do -- build the 128-bit product from native widening multiplies. Writing one would replace inlined
native-multiply codegen with differently-shaped inlined native-multiply codegen, not remove a call
or shrink the arithmetic. **Not building it -- there is no measured win available.**

**`div_round_i128()`: this is where the real cost is, and it genuinely calls out.** On Xtensa LX6:
0x3e0 bytes (comparable size on ARM), with four `callx8` call sites. Resolved via `objdump -dr`'s
relocations against the function's own literal pool (`.literal.div_round_i128_fn`), not guessed:
three of the four resolve to the cold `panic_const_div_by_zero` path (never taken in steady state,
since `d > 0` always holds by this recursion's own construction) -- but the fourth resolves to
**`__divti3`**, a real, generic signed-128-bit division routine from `compiler-builtins`. This is
the one true division in the Levinson-Durbin recursion (`k = -numerator/e`), called once per
iteration -- 10 iterations/frame at 50 frames/sec for codec2_3200 -- and it is a genuine software
bignum division, not something already optimized away.

**Why this isn't a narrowing question, and isn't attempted this pass.** Eliminating the `i128`
width here doesn't help the way it might have for `q_mul()` -- `__divti3` is already a generic
algorithm (handles the full 128-bit range), and the real win would come from an *algorithmic*
replacement exploiting this call site's own known structure (`e_q` is Q8.40 and strictly positive
by construction; the numerator is a compile-time-shaped left-shift), e.g. reciprocal-multiply against
a precomputed `1/e_q` with Newton-Raphson refinement -- not a width change at all. That is a real,
separate, higher-risk piece of work: **bit-exactness against the current `__divti3`-based result is
not achievable** with a reciprocal approximation (different rounding at the boundary is inherent to
the technique), so the acceptance bar has to be "diverges from the current implementation only at
the already-measured clamp-disagreement frames" (the same bar
`levinson_durbin_fixed_diverges_from_float_only_at_measured_clamp_disagreement_frames` already
enforces against float) -- which requires a real fixture-corpus run, including frame 273 (the
~3600x amplification frame), to even know whether a given reciprocal-multiply design clears it. That
run hasn't been done. **This document scopes it as a distinct follow-on; it is not started.**

**The `log2_lut`/`exp2_lut` gap is still open and still real.** `fixed_point.rs`'s own doc comment
(lines 22-54) already discloses that `quantise.rs`'s energy-quantization log/exp step is genuine
`f32` arithmetic, not fixed-point -- this study's own scope (`lpc.rs`'s `i64`/`i128` only) doesn't
touch it. Any framing of this work as "eliminated wide/software arithmetic for no-FPU targets" would
be misleading to an audience that acts on it (the M17/codec2 groups this was announced to) while that
gap remains -- worth stating plainly in any upstream message, not just in this internal doc.

## Not attempted this pass

The `div_round_i128()` reciprocal-multiply replacement (real opportunity, not yet built -- see
above); Question 2 (Chebyshev/LSP's own Q2.29 `i64`-to-`i32` narrowing, genuinely unmeasured);
Part 3's own real on-device cycle measurement (this pass used codegen inspection on the real target
ISA, not an on-device timer).
