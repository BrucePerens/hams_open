# Codec2 Fixed-Point Width-Reduction Study: Where Can 64/128-bit Narrow to 32?

## Status: Question 1 (i128 multiply) and the divide-side optimization are both measured
## and closed, 2026-09-04 -- neither was worth building. Question 2 (Chebyshev Q2.29,
## `i64`->`i32`) is measured, validated bit-exact against the real fixture corpus, and
## **implemented** -- `cheb_poly_eval_fixed` now runs as `i32` in `lpc.rs`, a real win on
## a function that (unlike Levinson-Durbin) genuinely runs in the live encoder. See "Real,
## measured findings" below -- this replaces the plan's own methodology item 4, which had
## recommended question 1 first as "highest-value, lowest-risk"; the value half of that
## claim is now measured false and is retracted -- the real win was in Question 2, not 1.
## Also discovered this pass: `levinson_durbin_fixed` is NOT wired into the real running
## `Encoder` at all (unlike `cheb_poly_eval_fixed`, which is) -- see "Real motivation"
## below.

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
occasionally. **Checked directly, 2026-09-04, and this framing needs a correction**:
`levinson_durbin_fixed` (and therefore every function this whole study concerns --
`q_mul`/`div_round_i128`) is not wired into the real running encoder at all. `Encoder::encode()`
in `mod.rs` calls the plain `f32` `levinson_durbin`; `levinson_durbin_fixed` is only ever called
from its own test module -- confirmed by grepping every `.rs` file and every example for a second
caller and finding none. **`div_round_i128`'s real, current cost in the running codec is exactly
zero cycles, not a small fraction of a core's budget** -- it doesn't run, because the fixed-point
encode path it belongs to hasn't been assembled yet (matching
`CODEC2_MOD_FIXED_POINT_PLAN.md`'s own stated scope: a characterization and a validated,
tested-in-isolation prototype, not yet an integrated fixed-point `Encoder`). This settles the
divide-side cost question this study was about to build a release-mode timing probe to answer --
more definitively than a probe of unused code could have, since "how long does it take" is moot
when the honest answer is "it never runs." Eliminating `i128` (question 1) is likely to be the highest-value, lowest-risk part
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
   checked (see "Real, measured findings" above) and is not being built. The apparent opportunity
   this measurement first surfaced -- replacing `div_round_i128()`'s `__divti3` call with a
   reciprocal-multiply technique -- turned out, on further checking, to be closed too: `div_round_
   i128` doesn't run in the real encoder at all (see "Real motivation" above), and even setting that
   aside, no sound acceptance bar exists for touching this specific division (bit-exactness isn't
   achievable, and the divergence-frame-set bar this document originally proposed is vacuous per the
   plan doc's own mantissa-width-sweep result). Neither half of this study's original "question 1"
   framing survived measurement. Question 2 (Chebyshev/LSP's own Q2.29 narrowing) remains open and
   independent of both -- it was never part of this framing to begin with.

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

**Why this isn't a narrowing question, and why it's now closed, not just deferred.** Eliminating
the `i128` width here doesn't help the way it might have for `q_mul()` -- `__divti3` is already a
generic algorithm (handles the full 128-bit range), and the real win would come from an
*algorithmic* replacement exploiting this call site's own known structure (`e_q` is Q8.40 and
strictly positive by construction), e.g. reciprocal-multiply against a precomputed `1/e_q` with
Newton-Raphson refinement -- not a width change at all. That would be a real, separate, higher-risk
piece of work, for two independent reasons neither of which changed by measuring harder:

1. **It optimizes code that doesn't run.** Per "Real motivation" above, `div_round_i128`'s real,
   current cost in the actual encoder is zero -- `levinson_durbin_fixed` isn't wired into
   `Encoder::encode()`. There is nothing to speed up until (if ever) a full fixed-point `Encoder`
   gets assembled, at which point this question should be re-asked in that real context, not
   answered speculatively now.
2. **There is no cheap, sound acceptance bar even if it did run.** The original plan here (below,
   struck through) proposed "diverges from the current implementation only at the already-measured
   clamp-disagreement frames." That bar doesn't discriminate: the plan doc's own mantissa-width
   sweep (16 through 32 bits) found the *identical* divergence count regardless of width, and
   `float32` diverges from `float64` on its own -- proving the clamp-disagreement frame population
   isn't a stable set a candidate avoids growing, it's determined by which frames' `k` lands near
   the threshold, and *any* change to the divide's rounding reshuffles membership. A candidate could
   pass "only diverges where clamp decisions differ" while silently trading one arbitrary ~0.9% of
   frames falling off the cliff for a different ~0.9% -- the test would be vacuous, not reassuring,
   on the single most numerically fragile step in the codec (both the clamp bifurcation and the
   documented 3600x amplification live here).

~~bit-exactness against the current `__divti3`-based result is not achievable with a reciprocal
approximation... the acceptance bar has to be "diverges from the current implementation only at the
already-measured clamp-disagreement frames"~~ -- retracted per point 2 above.

**Not pursued.** Both reasons are independent of each other and either alone would be sufficient;
together they close this cleanly. If a full fixed-point `Encoder` is ever built, this exact
question -- and a real acceptance methodology for touching this specific division, which does not
yet exist -- would need to be revisited from scratch in that context.

**The `log2_lut`/`exp2_lut` gap is still open and still real.** `fixed_point.rs`'s own doc comment
(lines 22-54) already discloses that `quantise.rs`'s energy-quantization log/exp step is genuine
`f32` arithmetic, not fixed-point -- this study's own scope (`lpc.rs`'s `i64`/`i128` only) doesn't
touch it. Any framing of this work as "eliminated wide/software arithmetic for no-FPU targets" would
be misleading to an audience that acts on it (the M17/codec2 groups this was announced to) while that
gap remains -- worth stating plainly in any upstream message, not just in this internal doc.

## Question 2, measured and implemented (2026-09-04): `cheb_poly_eval_fixed` is now `i32`, a real win

Unlike Levinson-Durbin's Q8.40 (which genuinely can't fit `i32` -- 8+40=48 bits, a hard
mathematical impossibility, not an untested question), the Chebyshev evaluation's own Q8.23/Q2.29
combination was already sized with `i32` in mind: `coef_q` was already typed `i32`, and
`cheb_poly_eval_fixed`'s own doc comment already asserted `\|T\|<=1`/`\|x\|<=1` "by construction."
The real, previously-unmeasured question was narrower than the original framing: not "do the T/x
*values* need `i64`" (they provably don't, by the same bound as `coef_q`), but "does `sum` (the one
value not structurally bounded to `[-1,1]`, accumulating up to 6 coefficient-scaled terms) also
stay within `i32`'s real range on real data."

**Measured directly** (`cheb_poly_eval_fixed_intermediate_values_real_measured_i32_fit_margin`, a
dense sweep against the real captured `codec2_pq_dump.txt` corpus, 2,896,000 evaluations): max
`\|x_q\|`/`\|T\|` ~2^29 (matches the format exactly), max `\|sum\|` ~2^29.94 -- both with real
margin under `i32`'s 2^31 range, not a knife's edge.

**Built a candidate** (`i32` storage for `x_q`/`T`/`sum`, matching `coef_q`; only the per-step
product widens to `i64` transiently -- the one native 32x32->64 multiply every target this port
cares about has in hardware -- narrowing back to `i32` immediately after each `>> CHEB_FRAC_BITS`,
the same shape `q_mul` uses one width class up) and **proved it bit-exact** against the prior
all-`i64` implementation across the same 2.9-million-evaluation corpus -- achievable here (unlike
`div_round_i128`) because this is deterministic shift/multiply/add arithmetic, not an approximation
technique with inherent rounding differences.

**Checked real codegen on both real target ISAs** (same `codec2_fixedpoint_codegen_check` crate,
extended with both variants): Xtensa LX6 -- **46 instructions / 117 bytes (`i32`) vs 98
instructions / 239 bytes (`i64`)**, roughly half; ARM Cortex-M4F -- 192 vs 320 bytes, same
direction. Zero calls either way on both (fully inlined, matching `q_mul`'s earlier finding).

**Why this one is a real win and the divide-side question wasn't**: `cheb_poly_eval_fixed` is
called from `find_next_root`, called from `lpc_to_lsp`, which **is** wired into the real
`Encoder::encode()` (unlike `levinson_durbin_fixed`) -- roughly 200 coarse-sweep calls plus
`LPC_ORD`(10)*`LSP_BISECTIONS`(6)=60 bisection calls per frame, ~260/frame at 50 frames/sec, ~13,000
calls/sec in the actual running encoder today. Halving a hot, currently-executing function's own
code size (and, on real hardware, its instruction-fetch/register-pressure cost) is a real,
currently-relevant win, not a speculative one.

**Implemented**: `cheb_poly_eval_fixed` in `lpc.rs` now runs this `i32` arithmetic directly (its
signature changed from `-> i64` to `-> i32`; `find_next_root`'s own `p_l`/`p_r`/`p_mid` types are
inferred, no other call-site changes needed). Full `codec2_3200` test suite re-run after the swap,
46/46 passed, including the real reference-decoder cross-check
(`decoder_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream`) and the
full encode/decode round trip -- not just the Chebyshev-specific tests.

## A validated design for a future Levinson-Durbin optimization, not yet built (Bruce's own idea, 2026-09-04)

Raised while discussing why the divide-side question above was closed: since `e` in Levinson-Durbin
is monotonically non-increasing across iterations (multiplied by `(1-k^2) <= 1` each step),
"heading into small-`e` territory" is a one-directional, cheap-to-check condition -- not something
that flickers. A real design this suggests for a future fixed-point `Encoder` (should one ever be
assembled): run the recursion in a cheaper, narrower format while `e` stays comfortably large; the
moment `e` crosses a threshold that would make `1/e` amplification exceed the narrow format's error
budget, convert the running state (`a[]`, `e`) up to the already-validated wide Q8.40
representation -- an exact, lossless widening -- and continue from there for the rest of that frame,
since `e` won't climb back out. This is the same fast-path/slow-path shape IEEE-754 hardware uses
for subnormals, and critically **it doesn't trade away correctness for speed** -- if the wide path,
when triggered, is the exact same Q8.40 arithmetic already proven to handle frame 273, the final
output is identical to today's all-Q8.40 implementation; only the *average* cost drops (worst-case
frames still pay full width). Not built this pass, for the same reason the divide-side question
wasn't pursued further: `levinson_durbin_fixed` isn't wired into the real encoder yet, so there is
nothing running to speed up today. Recorded here so it isn't re-derived from scratch if/when a real
fixed-point `Encoder` gets built.

## Not attempted this pass

Part 3's own real on-device cycle measurement (this pass
used codegen inspection on the real target ISA, not an on-device timer, though for `div_round_i128`
specifically this is now moot -- see "Real motivation" above); assembling a full fixed-point
`Encoder` (a real, separate, much larger undertaking this study does not scope at all).
