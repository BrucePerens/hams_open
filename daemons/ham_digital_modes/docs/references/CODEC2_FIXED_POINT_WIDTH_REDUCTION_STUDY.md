# Codec2 Fixed-Point Width-Reduction Study: Where Can 64/128-bit Narrow to 32?

## Status: proposal only, 2026-09-04 -- no code written against this plan yet

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
2. **Priority between question 1 (eliminate i128, lower risk) and question 2 (narrow i64 to i32,
   higher risk, real precision headroom already measured as a concern)** -- this plan recommends
   question 1 first, but sequencing both together or deferring question 2 entirely pending
   question 1's own real results is Bruce's own call once question 1's real data exists.

## Not attempted this pass

No code written, no measurement taken yet -- this document scopes the real study Bruce asked for;
executing it (building the narrowed variants, running them against the fixture corpus, and, if
real ESP32 access exists, a real on-target measurement) is the next, separate piece of work.
