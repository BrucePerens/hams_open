# Draft note for the M17 project re: Codec2-mod's bit-exactness claim

**Status: drafted, not filed.** This is a ready-to-review writeup for Bruce's own judgment on whether
and how to raise it with the M17 project (e.g. as a GitHub issue on `M17-Project/Codec2-mod`, a forum
post, or direct outreach) -- filing anything upstream is outside this codebase and is Bruce's own call,
not something this session does on its own. Checked `gh api repos/M17-Project/Codec2-mod/issues`
directly on 2026-09-04: the repo has zero issues, open or closed, so this has not already been raised.

Full technical writeup, including the exact mechanism and both real tests that confirm it, lives in
`CODEC2_MOD_FIXED_POINT_PLAN.md` (same directory), in the section "A real, significant risk found by
actually testing Levinson-Durbin against real data" and its "upstream bit-exactness claim" subsection.
This file is a shorter, outward-facing version of the same finding, written for people outside this
project who haven't read that document.

---

## Suggested issue title

Levinson-Durbin's `|k|>1` clamp is a real bifurcation point -- bit-exactness may be corpus/toolchain
dependent, not universal

## Suggested issue body

Thanks for this project -- a clean, minimal 3200bps extraction is genuinely useful, and the
bit-exactness verification against reference `libcodec2` is a great foundation to build on.

While investigating this codebase for a fixed-point port (unrelated to this fork directly -- a
downstream project exploring embedded targets without an FPU), I found and confirmed a real numerical
property of `levinson_durbin()` worth flagging, since it bears on the strength of the bit-exactness
guarantee this fork advertises.

**The finding**: `levinson_durbin()`'s own safety clamp (`if (fabsf(k) > 1.0f) k = 0.0f;`) is a genuine
bifurcation point, not just a boundary condition. On a frame where the true reflection coefficient sits
close to +-1, an arbitrarily small rounding difference earlier in the recursion can push the computed
`k` from just inside the valid range to just outside it (or vice versa) -- and crossing that boundary
doesn't produce a small output difference, it flips whether the clamp fires at all. When it does, every
later iteration recurses from a completely different `a_prev[]` state, and the final LPC coefficients
for that frame can differ by an order of magnitude, not a rounding-sized amount.

**This isn't hypothetical or fixed-point-specific.** I tested it directly: took 2532 real R[]
autocorrelation vectors from real speech (5 files, ~50s total, resampled to 8kHz), ran them through two
implementations of the *identical* recursion -- one in plain `float` (32-bit, matching this fork's own
precision), one in `double` (64-bit) -- with no quantization, no fixed-point, no source changes beyond
the type. **1 of 2532 real frames (0.04%) still diverges by more than 1.0 in at least one LPC
coefficient, with 10 more (0.4%) showing a smaller but real perturbation.** That's `float` alone, at
your own fork's own precision, disagreeing with a higher-precision run of the mathematically identical
formula, purely from where intermediate rounding happens to land relative to the clamp.

**Why this matters for the bit-exactness claim specifically**: the README states bit-exactness with
reference `libcodec2` "has been verified using identical input signals and byte-for-byte comparison of
encoded frames." I compared your refactored `levinson_durbin()` (`src/lpc.c`) against upstream's own
(`codec2/src/lpc.c`) line by line -- the refactor from a 2D iteration-history array to two rolling 1D
arrays is arithmetically transparent; every operation reads the same operands in the same order with
the same expression shape, so the refactor itself isn't the risk. The risk is that the underlying
recursion (unmodified since Makhoul 1975, present in upstream too) sits close enough to this clamp
boundary that *any* difference in floating-point rounding path -- a different compiler, a different
optimization level, a different libm, or critically, cross-compilation to the embedded target this fork
is aimed at (the STM32F405 numbers in your own README) versus whatever host compiled the verification
build -- could flip a real, if rare, frame's clamp decision and produce a bitstream that isn't
byte-for-byte identical to what was verified on the dev host.

**What I'm not claiming**: I have no evidence this fork's actual STM32F405 build currently diverges
from the verification build -- I haven't tested that specific comparison, only the general
float32-vs-float64 sensitivity of the shared algorithm. This is a "here's a real numerical cliff your
bit-exactness claim walks near, worth being aware of and maybe worth a note in the README" report, not
a "your code is broken" report.

**Possible mitigations**, for what it's worth (not tested against real audio quality by me, just
sketched as options): a smoother transition at the clamp boundary (blending `k` toward the limit rather
than a hard snap to exactly the reference's own snap point) would reduce how often a tiny rounding
difference changes *which side* of a discontinuity a frame lands on, at the cost of changing the
reference algorithm's own behavior slightly -- that trade-off is a real product decision, not an
obvious win, and I'd defer to your own judgment on whether it's worth pursuing.

Happy to share the test harness and the 2532-vector real-speech corpus if useful.
