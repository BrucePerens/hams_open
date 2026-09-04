# Codec2-mod: A Fixed-Point Domain Characterization and Implementation Plan

**Relocated here 2026-09-04, from `hams_com/docs/proposals/` (that repo's private planning area) to
this LGPL-3.0-or-later crate's own `docs/references/`, alongside `FT8_FT4_GFSK_WAVEFORM_SPEC.md`** --
this document analyzes and quotes real source from Codec2-mod, an LGPL-2.1-only project for most of
its tree (`github.com/M17-Project/Codec2-mod`, itself derived from David Rowe's LGPL-2.1-only
`drowe67/codec2`; its bundled KISS FFT is separately BSD-3-Clause), so it belongs alongside the
LGPL-licensed code it studies, not in a private repo. The actual Codec2-mod source this document was
checked against is vendored, unmodified, at `../../vendor/codec2-mod/` (per-file license breakdown
and a real, still-open LGPL-2.1-only-vs-LGPL-3.0-or-later compatibility question -- not yet resolved,
not yet load-bearing since nothing here is linked into this crate's build -- both in
`../../vendor/codec2-mod/VENDORED_FROM.md`).
See `../../vendor/codec2-mod/VENDORED_FROM.md` for the exact commit vendored.

**Status: research/planning proposal, 2026-09-04. Bruce's own direct request: look at the M17
project's Codec2-mod work, and characterize the domains of all the math in the codec sufficiently to
make a fixed-point version. Grounded in a real read of the actual M17 source (cloned and read in full
this session, `github.com/M17-Project/Codec2-mod`, commit at clone time), not upstream Codec 2's own
much larger and more sprawling repo. This document's own original "no code written here" line is now
stale, not silently corrected but left visible per this project's own established convention -- see
"Real measured ranges" and "First real fixed-point primitive, built and validated" below for what
this became: the validation harness's own first honest step was actually run against the real float
reference (finding and correcting one of this proposal's own earlier claims), and the "easiest stage"
this proposal identified (the phase-accumulator/DDS pattern) was actually built and validated in real
C, not just described.**

## What Codec2-mod actually is, verified directly against the M17 project's own announcement and source

Per the M17 project's own 2025-12-29 announcement
(`m17project.org/2025/12/29/codec2-mod-released-for-testing/`): Codec2-mod isolates upstream Codec
2's 3200 bit/s mode into a small, standalone, C-only repository (no Octave test benches, no modems),
with fully static memory allocation (including KISS FFT's own scratch buffers -- no `malloc` anywhere
in the real cloned source, confirmed directly), general code clean-up, and two named DSP-level
changes: faster trig approximations in the LPC-LSP path (`fast_cosf`/`fast_acosf` replacing
`cosf`/`acosf`), and a polyphase-filter-bank re-implementation of the pitch estimator's decimating
FIR (`nlp_fir_poly` in `nlp.c`, confirmed directly -- 5 phases x 10 taps, replacing a single
48-tap direct-form FIR). Measured on real STM32F405 hardware (168MHz): ~7.021s -> ~6.754s for 1000
encode iterations. **It is still entirely `float`-based -- confirmed by reading every `.c` file in
the repo, not assumed from the announcement's own silence on the topic.** This is real, useful,
already-landed embedded-optimization work, but it does not touch the actual question this proposal
addresses.

## Why fixed-point at all -- an honest treatment, not just assumed worth doing

Before characterizing anything, the real prior-art check this session did turned up a directly
relevant, load-bearing fact: **David Rowe has stated directly, on the Codec 2 mailing list, that
Codec 2's DSP is "all floating point," that he'd be willing to help someone attempt a fixed-point
port, and that he expects fixed-point to need *more* MIPS than float, not fewer** -- floating-point
hardware on a modern Cortex-M4F/M7 (the STM32F405 Codec2-mod was benchmarked on already has one) is a
solved, fast, single-cycle-throughput problem; fixed-point requires manual scaling/shift bookkeeping
at every operation, which is real, ongoing engineering cost with no guaranteed speed win. **This means
"faster" is not automatically the right motivation for this work**, and this proposal should not
pretend otherwise. The real, honest reasons fixed-point is still worth characterizing:

1. **A genuinely FPU-less target**, not just "a microcontroller" -- Codec2-mod's own STM32F405
   benchmark target already has a hardware FPU. The real win is a smaller/cheaper/lower-power part
   with no FPU at all (Cortex-M0/M0+ class, or a true 8/16-bit part), where float means a slow
   software-emulated library, not a one-cycle instruction -- there fixed-point genuinely is faster,
   not just different.
2. **Bit-exact reproducibility across implementations.** This is the same reason ITU-T standard
   codecs (G.729, G.723.1, GSM-EFR/AMR) mandate fixed-point reference code at all: float rounding can
   differ subtly across compilers/FPU revisions/optimization levels, while integer fixed-point
   arithmetic is exactly reproducible bit-for-bit given the same inputs. For an interoperability
   protocol (which is exactly what a ham-radio digital-voice codec is, M17 included), this is a real,
   independent reason to want a fixed-point reference implementation even on hardware with a perfectly
   good FPU.
3. **Code size** on a flash-constrained part -- avoiding a soft-float library pull-in.

Bruce's own read on why Rowe never did this himself, offered directly: he "just did not want to spend
the time when CPUs with an FPU cost $5." The evidence found this session is consistent with that --
the Blackfin mailing-list attempt below is from 2010-2013, well before Cortex-M4F-class FPU hardware
was a commodity part; the economics have only moved further in that direction since, which explains
continued disinterest from Codec 2's own maintainer without implying the port itself is intractable.

None of these were checked against a concrete named target in this codebase's own proposal backlog --
worth being honest that this is exploratory, foundational research, not unblocking an already-planned
hams.com embedded product the way `ROTOR_CONTROL.md` or `RTTY_DIGITAL_MODE.md` unblock named backlog
items.

## The full pipeline, and what it actually computes -- from the real source, not upstream's docs

Read directly, function by function (`codec2_mod.c`, `analysis.c`, `nlp.c`, `lpc.c`, `quantise.c`,
`interp.c`, `synthesis.c`, `util.c`): encode is windowing -> NLP pitch estimate (FFT-based, on the
*squared* signal) -> two-stage harmonic-sum pitch refinement (FFT-based, on the *raw* windowed
signal) -> LPC analysis (autocorrelation + Levinson-Durbin) -> LPC-to-LSP (Chebyshev polynomial root
search via bisection) -> scalar quantization of Wo and energy -> delta-VQ of the 10 LSPs against a
fixed codebook (`delta_lsp_cb.c`). Decode is the reverse plus harmonic sinusoidal synthesis:
LSP-to-LPC -> LPC-to-magnitude-spectrum (with a postfilter/formant-sharpening gain normalization
step) -> phase synthesis from a running phase accumulator -> harmonic-sum FFT synthesis with
overlap-add.

## The single most important characterization finding: this pipeline does NOT have one natural
## numeric domain -- it has at least three, and they must be treated differently

**Speech samples are carried in native int16 magnitude scale throughout, not normalized to +-1.0**
(confirmed: `c2->Sn[i + M_PITCH - N_SAMP] = speech[i]` assigns a raw `int16_t` directly into a
`float` array, no `/32768.0f` anywhere in the read path). This matters enormously for fixed-point
planning, because every squaring/FFT-power operation downstream compounds that scale:

- **NLP pitch estimation operates in an enormous, and *unbounded-by-any-fixed-Q-format*, dynamic
  range.** `nlp->sq[i] = Sn[i]*Sn[i]` alone reaches up to 32767^2 ~ 1.07x10^9 for a full-scale sample.
  That gets FFT'd (`PE_FFT_SIZE = 512`) and the result is squared again (`Fw[i].r = Fw[i].r^2 +
  Fw[i].i^2`, `nlp.c` line ~208) -- a conservative bound on that final power value is on the order of
  10^20-10^23 depending on real signal statistics. **No single fixed Q-format can represent both
  "silence" and "this value" without either catastrophic underflow at one end or overflow at the
  other.** This stage needs either block-floating-point (a per-frame shared exponent, renormalized
  each frame -- the standard technique in fixed-point audio codecs for exactly this problem) or a
  log-domain representation, not a naive linear Q-format port.
- **The LPC-analysis path has a much tamer *maximum* than the NLP path, because it operates on a
  *normalized* analysis window -- but real measurement (see "Real measured ranges" below) found its
  overall dynamic range is not actually fixed-point-friendly.** `make_analysis_window` (`analysis.c`)
  explicitly scales the window by `1.0f / sqrtf(m * FFT_ENC)` (comment: "normalize (bit-exact with
  original)"), which brings windowed-sample magnitudes down to roughly the 100s at full scale, not
  the tens-of-thousands -- and a first back-of-envelope pass estimated `autocorrelate()`'s `R[0]`
  landing in the ~10^6-10^7 range for a full-scale frame, comfortably inside a 32-bit accumulator.
  **Real measurement across 50.8 seconds of real speech corrected this**: `R[0]`'s real range is
  4.15x10^-6 to 8.60x10^5 -- the maximum estimate was right, but real speech's own quiet passages
  push the minimum 11 orders of magnitude below it, which a linear accumulator sized for the maximum
  cannot represent with any real precision. The genuinely useful asymmetry that survives real
  measurement: the pitch-detection and LPC-analysis paths have very different *maximum* magnitudes
  (one pre-scaled by a normalizing window, one not), but **both need the same class of
  block-floating/log-domain treatment for their own dynamic range**, not "one is tame, one isn't."
- **Everything downstream of LSP quantization is small, well-bounded, and fixed-point-friendly by
  construction.** `Wo` (fundamental frequency, radians/sample) is hard-bounded to
  `[W0_MIN, W0_MAX] = [2*pi/160, 2*pi/20]` ~= `[0.0393, 0.3142]` and is *already* linearly quantized to
  a 7-bit index in the real bitstream (`encode_Wo`/`decode_Wo`) -- that quantizer's own arithmetic is
  effectively a fixed-point representation already, just not currently reused as the codec's internal
  working format. LSP frequencies live in `[0, pi]` radians by construction. The phase accumulator
  (`ex_phase[0]`, `synthesis.c`) is wrapped into `[-pi, pi]` every frame by explicit code. These are
  all natural, narrow-range fits for a standard Q-format (e.g. Q2.14 or Q2.30), no special handling
  needed beyond routine fixed-point angle representation -- the same pattern every hardware DDS/NCO
  already uses for phase accumulation, extremely well precedented.

## Per-stage characterization table

| Stage (function) | Quantity | Real domain, from the actual code | Fixed-point risk / recommended treatment |
|---|---|---|---|
| `nlp()` | `sq[i]`, `Fw[i].r/i` power | int16^2 up through a 512-pt FFT power, ~10^9 to ~10^23 | High risk -- block-floating-point (per-frame renormalized exponent), not linear Q-format |
| `nlp()` | notch filter (`mem_x`, `mem_y`, `COEFF=0.95`) | Simple IIR on the same huge-range `sq[]` signal | Inherits the block-floating treatment above; coefficient `0.95` itself is trivially fixed-point (Q15) |
| `nlp()` | pitch period / `best_f0` | Bounded, `P_MIN..P_MAX` = 20-160 samples, ~50-400Hz | Low risk -- narrow range, ordinary fixed-point |
| `autocorrelate()` | `R[0..10]` | **Real measured range (see "Real measured ranges" below): 4.15x10^-6 to 8.60x10^5 across real speech** -- a real ~10^11 span, not just a bounded maximum | **Revised to high risk** after real measurement -- the maximum alone fits a 32-bit accumulator, but the real quiet-frame minimum is 11 orders of magnitude below it; needs the same block-floating/log-domain treatment as the NLP power path, not a plain linear accumulator |
| `levinson_durbin()` | reflection coeff `k`, error `e` | `k` in `[-1,1]` by construction (code has an explicit safety clamp for pathological input); `e` is monotonically non-increasing, multiplied by `(1-k^2)` each of 10 iterations. **Real measured risk (see "A real, significant risk" below), not just a shrinking-`e` concern**: the `\|k\|>1` clamp is a genuine bifurcation point -- 0.9% of 2532 real captured frames diverged by an order of magnitude in a block-floating simulation, and the *same* divergence (0.04% of frames) occurs between plain `float32` and `float64` with no fixed-point involved at all, proving this is inherent to the algorithm's own sensitivity at that boundary, not fixable by adding mantissa bits | **Highest numerical risk in the whole codec, confirmed and precisely characterized, not just flagged.** Block-floating `e` (rescale and track a shared exponent each iteration, the ITU-T G.729/GSM-EFR pattern -- see "Real precedent" below) is still the right treatment for the *shrinking-magnitude* half of this risk, but does **not** by itself close the clamp-boundary bifurcation -- that needs a separate, real decision (accept the measured real divergence rate, matching what `float32` already ships with, or add a smoothing/stabilization step at the clamp itself, which changes the reference algorithm and needs its own audio-quality validation) |
| `lpc_to_lsp()` / `cheb_poly_eva()` | `x` (search variable) | Exactly `[-1, 1]` by construction (Chebyshev domain) | Low risk for `x` itself -- ideal Q15/Q31 fit |
| `lpc_to_lsp()` | `P[]`/`Q[]` polynomial coefficients, `psum` | Derived from LPC coefficients (`a[i] + a[11-i]`), magnitude not hard-bounded by the algorithm, empirically bounded by LPC coefficient magnitude (roughly single-to-low-double digits for a stable filter) | Moderate risk -- needs empirical characterization across real speech (not just assumed bounded), and needs a Q-format with real integer headroom, not a pure fractional format |
| `lpc_to_lsp()` bisection | sign comparisons only (`psumr <= 0 && psuml >= 0`, etc.) | -- | **A genuinely favorable finding, not just a risk list**: this whole root-search only cares about the *sign* of `psum`, not its magnitude -- meaning it's naturally tolerant of coarse quantization as long as sign is preserved near a genuine zero crossing. Don't over-engineer precision here. |
| LSP frequencies (post-`fast_acosf`) | `freq[j]` | Exactly `[0, pi]` by construction | Low risk -- same angle-domain Q-format as `Wo` |
| `encode_Wo`/`decode_Wo` | `Wo` | `[W0_MIN, W0_MAX]`, already linearly quantized to 7 bits in the real bitstream | Already effectively fixed-point in the reference format -- reuse this quantizer's own arithmetic as the internal representation rather than inventing a separate one |
| `encode_energy`/`decode_energy` | `e` (linear) / `e_db` | `e_db` in `[E_MIN_DB, E_MAX_DB] = [-10, 40]` dB, linear `e` in `[10^-1, 10^4]` -- a real 5-decade span | Moderate risk -- the *dB* domain is narrow and fixed-point-friendly; the *linear* domain isn't. The reference code already quantizes in dB -- keep the internal fixed-point representation in the log/dB domain too, converting to linear only where unavoidable, via a fixed-point `pow10`/`log10` LUT (well precedented, see below) |
| `aks_to_mag2()` | `A2[i]`, `invA2 = 1/A2[i]` | `A2` bounded by a short (11-tap) filter's own power spectrum, but can approach its `1e-6f` floor near an LPC spectral null (common for nasal/resonant frames) -- `invA2` can then spike toward its floor-bounded max (measured 244,880) | **Second-highest risk in the codec -- validated 2026-09-04, see that section below.** An 8-bit log2/exp2-LUT replacement for the reference float code's own `logf`/`expf` treatment of `R^(2*BETA)` was tested against 5078 real captured `(E, A2[], A2g[])` frames: max relative error in `gain` 8.25e-7, RMS 3.64e-7 (a handful of float32 ULPs), with smooth, resolution-dependent degradation (confirmed via a deliberately coarser 4-bit LUT) rather than any discontinuity. Unlike Levinson-Durbin, this stage has no open numerical-soundness question left -- only Q-format engineering detail for the surrounding fixed-point arithmetic. |
| `aks_to_mag2()` | `gain = E * e_before / e_after` | `E` already log-quantized (see above); `e_before`/`e_after` inherit the `invA2` risk | Same log-domain treatment as above |
| `phase_synth_zero_order()` | `ex_phase[0]`, harmonic phase `phi0*m` | Wrapped to `[-pi, pi]` every frame by explicit code | Low risk -- standard DDS/NCO phase-accumulator pattern, extremely well precedented in fixed point (this is arguably the easiest stage in the whole codec) |
| `sample_phase()`/`phase_synth_zero_order()` | `H[m]` (LPC synthesis filter response at a harmonic) | **Corrected 2026-09-04, this row's original claim was wrong -- checked the real source, not re-derived from structure.** `sample_phase()` never inverts or divides `A[b]`: `H[m].r = A[b].r; H[m].i = -A[b].i;` is a plain conjugate assignment, so `H[m]`'s own magnitude is bounded by the same `sqrt(A2[])` range already measured (~0.002 to ~78.5), not by `invA2`'s ~9-decade spike. `H[m]` is then used purely for *phase*: `A_[m] = H[m]*Ex[m]` (with `Ex[m]` unit-magnitude) feeds only into `fast_atan2f(A_[m].i, A_[m].r)`, which is scale-invariant in its inputs. The one real division in this path, inside `fast_atan2f` itself (`r = (x-abs_y)/(x+abs_y)`), is a self-normalizing Padé-style ratio provably bounded to `[-1, 1]` for any real `x`, `abs_y >= 0` -- structurally immune to the unbounded-dynamic-range problem `invA2` actually has. | Low risk, not the same problem as `aks_to_mag2()` -- ordinary bounded fixed-point suffices for `H[m]` itself; `fast_atan2f`'s own `[-1,1]`-bounded ratio needs no log-domain treatment at all, just a real polynomial/LUT approximation of the arctangent shape, independent of this proposal's log-domain work. |
| `postfilter()` | `e = 10*log10f(...)`, `bg_est` | dB-domain accumulator, narrow real range in practice (compared against `BG_THRESH=40.0`) | Low-moderate risk -- needs a fixed-point `log10` LUT (well precedented), otherwise narrow and safe |
| `ear_protection()` | output magnitude clamp vs. `30000.0f` | int16-scale, already the codec's native output domain | Low risk -- direct int16 comparison, no conversion needed at all |
| KissFFT itself (`kiss_fft.c`/`kiss_fftr.c`) | FFT butterfly arithmetic | -- | **Not a novel design problem.** The vendored KissFFT in this exact repo already ships a compile-time `FIXED_POINT` mode (`kiss_fft.h`: `int16_t` or `int32_t` scalar via `#define FIXED_POINT 16`/`32`, with `sround()`/shift-based scaling already implemented in `_kiss_fft_guts.h`). This is "enable the flag and re-tune scaling per call site," not new DSP design. |

## Has this actually been tried before? Real evidence, checked directly -- not left as an open question

Two real, independent historical data points, found via direct search rather than assumed one way or
the other:

**A real attempt that stalled, ~2010-2013** (`freetel-codec2` mailing list, "Fixed Point
implementation of Codec2"): Steve Strobel tried running Codec2 on a real Analog Devices Blackfin DSP
(BF537, 500MHz). This was cross-compiling the existing *float* C code with workarounds (Blackfin's
own strict alignment requirements caused real crashes David Rowe himself helped diagnose), not a
proper fixed-point rewrite -- and even after real optimization work, encoding 3 seconds of audio took
17-21 seconds, 6-7x too slow for real time. No definitive solution emerged. **This is real negative
evidence about the "port the float code with workarounds" approach specifically, not about a proper
domain-characterized fixed-point rewrite** -- it never attempted the block-floating/log-domain
techniques this proposal's own table above calls for.

**A real, independent, published success, 2021**: Jamieson, Sampath Kumar, Nacif, and Ferreira,
"Analyzing a Low-bit rate Audio Codec - Codec2 - on an FPGA" (IEEE CSCI 2021, PDF read directly this
session). An unrelated academic team fully reimplemented Codec2's MODE2400 encoder (a different bit
rate than Codec2-mod's 3200, but the same core algorithm family -- FFT, NLP pitch estimation, LPC
analysis, LPC-to-LSP, quantization) as synthesizable fixed-point Verilog on a real Intel Cyclone IV
FPGA, and it worked: **"qualitatively the same in terms of hearing the spoken transmission,"** an
average bit-level difference of 6.55 bits per 48-bit frame versus the float reference, explicitly
attributed to **"repetitive multiplications and additions in the modules for Fourier Transform and
auto-correlation"** -- independent, real-world confirmation of exactly the two highest-risk areas this
proposal's own table above identified from reading the source directly, before finding this paper.
Their chosen representation: a custom 32-bit format (1 sign + 15 exponent + 16 mantissa bits -- a
software/hardware "mini-float," not a pure linear Q-format) with an 80-bit extended format for
squaring operations that overflow the 32-bit range, using CORDIC for `cos`/`acos`/`sincos` and
Newton-Raphson for division/`sqrt` -- unremarkable, well-established hardware DSP technique, not
anything novel. Performance on an unpipelined, decade-old, low-end FPGA (50MHz) was already close to
real-time (3.45s to process 3s of audio) with the paper's own authors identifying a well-understood,
concrete path (two pipeline stages) to close the remaining gap. They estimated ~15 days of real
engineering effort per additional bit-rate mode beyond the one they built, with most core modules
(FFT, pitch estimation, LPC analysis, LPC-to-LSP) directly reusable across modes -- a real, credible
effort data point from people who actually did comparable work, not a guess.

## Real precedent to build on, not re-derive from scratch

Two real, concrete, well-documented toolkits found directly this session, not assumed to exist from
general DSP knowledge:

1. **ITU-T's own G.729/G.723.1 fixed-point reference C code** (published directly by ITU-T,
   `itu.int/rec/T-REC-G`) is the standard, decades-proven toolkit for exactly this class of problem --
   a "basic operators" library (`L_mac`, `L_mult`, saturating add/sub, `norm_l` for
   block-floating-point renormalization, fixed-point `div_s`, table-driven `log2`/`pow2`) built
   specifically for LPC analysis, Levinson-Durbin, and LSP conversion in 16/32-bit fixed point on
   real embedded DSPs. This is the concrete template for the Levinson-Durbin and `aks_to_mag2`
   log-domain risk areas identified above -- their `norm_l`-style renormalization is exactly the
   "block-floating `e`" treatment Levinson-Durbin needs here.
2. **KissFFT's own vendored `FIXED_POINT` mode**, already present in this exact M17 source tree
   (confirmed directly, not assumed) -- the FFT stages don't need new fixed-point design work, just
   enabling and re-validating.

## Validation harness design: instrumenting both codecs stage-by-stage to localize divergence

Comparing only final decoded audio between the float reference and a fixed-point prototype tells you
*that* something diverged, not *where* -- by the time an error introduced in, say, `levinson_durbin()`
reaches an audio sample, it has been reshaped by LSP conversion, quantization, interpolation, LPC
reconstruction, and harmonic synthesis, and the resulting audio difference carries no signal about
which stage actually broke. A silent numeric bug (wrong shift, wrong rounding mode, an accumulator
that overflows without crashing) doesn't announce itself -- it produces audio that decodes to
*something*, often plausible-sounding, sometimes just subtly wrong. This is a real, specific risk for
this project given who would be doing the implementation work (see the session's own prior discussion
of AI-specific failure modes): the failure mode is exactly the one hardest to self-catch without
deliberately-built instrumentation, since the code compiles, runs, and produces audio-shaped output
regardless of whether it's correct. The fix is turning "does the audio sound okay" (a perceptual
judgment this proposal's own author can't make directly) into "does each stage's fixed-point output
diverge from the float reference by more than that stage's own chosen format predicts" (a fully
mechanical, automatable check).

### What to instrument

Cross-wire both codecs to run on the same input, frame by frame, and log both the float value and the
fixed-point value (converted back to a comparable representation) at every named intermediate quantity
already identified in the per-stage table above -- not just inputs/outputs of each function, but the
specific at-risk internal values:

- `nlp()`: `best_f0`/pitch period, and separately the raw `Fw[i].r/i` power values feeding the peak
  search (since the peak-search *decision*, i.e. which bin wins, matters more than the power values'
  own absolute magnitude -- see "decision divergence" below).
- `autocorrelate()`: `R[0..10]`.
- `levinson_durbin()`: reflection coefficient `k` and error `e` at *every one of the 10 iterations*,
  not just the final `ak[]` -- this is the stage flagged as highest-risk specifically because `e` can
  shrink toward zero partway through the recursion, and a bug there could be invisible in the final
  coefficients if it happens to self-correct, or could compound silently across the remaining
  iterations.
- `lpc_to_lsp()`: the `roots` count (does root-finding fail equally often in both paths?) and the
  final LSP frequencies.
- Quantizer indices: `Wo_index`, `e_index`, `lspd_indexes[0..9]` -- exact-match rate, not distance,
  since these are the actual transmitted bits.
- Decoder side: `lsp_to_lpc()`'s reconstructed `ak[]`, `aks_to_mag2()`'s `A2[i]`/`invA2`/`gain`, the
  final `model->A[m]` amplitudes, `phase_synth_zero_order()`'s `ex_phase[0]` and per-harmonic `phi[m]`,
  and the final `Sn_[]` samples.

### Metric per stage, matched to that stage's own domain -- not one blanket number

A single "percent difference" metric is wrong for most of these quantities, because they don't share a
domain (see the three-numeric-regimes finding above):

- **Linear-scale quantities** (`R[]`, `A2[]`, samples): SNR in dB,
  `20*log10(|ref| / |ref - test|)` -- the natural way to express "how much noise did the fixed-point
  path add relative to the signal itself," and directly comparable across stages despite their wildly
  different absolute magnitudes.
- **Angle-domain quantities** (`Wo`, LSP frequencies, `ex_phase`, `phi[]`): absolute error in radians,
  computed via a *wrapped* angle difference (`atan2(sin(a-b), cos(a-b))`-style), not naive
  subtraction -- `ex_phase` in particular wraps every frame by explicit code in the reference, and a
  naive difference across a wrap would falsely report a near-`2*pi` divergence for a value that's
  actually correct.
- **Quantizer indices**: exact-match rate, with "off by exactly one codebook/quantizer level" tracked
  *separately* from "off by more than one." The former is expected, mostly-benign boundary noise (a
  value legitimately close to a quantizer decision boundary can land on either side of it in float vs.
  fixed-point without either being "wrong"); the latter is a real signal something upstream is
  meaningfully off.
- **Decision points, not just values** -- `nlp()`'s pitch-bin winner and `est_voicing_mbe()`'s
  voiced/unvoiced flag are the two clearest examples: log whether the two paths made the *same
  decision*, independent of how close the underlying scores were. A near-tie decision flipping
  between paths is a real, expected, low-severity divergence (both paths are "right," they just broke
  a tie differently); a decision that flips on a signal where the winning margin was large in the
  float reference is a real bug.

### The genuinely load-bearing idea: compare against the *predicted* quantization floor, not against zero

Every stage will diverge from the float reference by *some* nonzero amount, always -- that's normal
and expected, not a bug. A chosen Q-format has an inherent, computable noise floor (a Q15 fractional
format has an inherent ~2^-15 relative quantization step, for instance). The useful signal isn't "is
there divergence" (always true) but **"is the observed divergence larger than what the chosen format's
own arithmetic predicts it should be."** Compute that predicted floor per stage from the actual chosen
word width/format, and flag any stage whose measured divergence exceeds it by a meaningful margin (a
real, un-budgeted-for bug) versus tracks it closely (expected, harmless rounding). This is what turns
"float and fixed don't match" (true of every real fixed-point port, uninformative on its own) into a
specific, actionable, debuggable finding.

### Cross-substitution to localize *new* error from *inherited* error

For any stage N that shows divergence, it's not automatically clear whether stage N itself introduced
new error or is faithfully propagating error that already existed upstream. Run stage N's fixed-point
implementation twice per frame: once fed by its own upstream fixed-point output (the real end-to-end
path), and once fed by the *float reference's* upstream output, cross-injected directly into stage N's
fixed-point inputs. If the second run tracks the float reference much more closely than the first,
stage N is faithfully inheriting upstream error, not adding much of its own -- look upstream instead.
If both runs show similar divergence from the float reference, stage N itself is the real source. This
is standard differential-testing practice (sometimes called golden-input substitution), not novel to
this proposal, but directly answers "which of the ten stages do I actually need to fix" instead of
just "something upstream of the final audio is wrong."

### Aggregate across a real, deliberately adversarial corpus -- and track tails, not averages

Matching this project's own established discipline (`SSB_SPECTRAL_TUNING_RESEARCH.md`,
`PSK31_SCANBANK_ARBITRATION_RESEARCH.md`, and the FPGA paper's own real-corpus methodology above): run
this instrumentation across hundreds-to-thousands of real frames, not one clip, spanning quiet/loud,
voiced/unvoiced, male/female, and specifically resonant/nasal content (the exact content class that
stresses `levinson_durbin()`'s shrinking `e` and `aks_to_mag2()`'s near-null `invA2` -- both flagged as
content-dependent risks in the per-stage table, not uniform-across-all-audio risks). **Report the
worst-case tail (max, p99) per stage-metric, not just the mean.** A mean divergence across a large
corpus can completely hide one catastrophic frame in ten thousand -- exactly the shape of failure the
two flagged high-risk stages could plausibly produce (a rare, strongly-resonant frame that happens to
drive `e` very close to zero, or an LPC spectral null landing exactly on a harmonic).

### Closing the loop: this is also how the "can't listen" limit gets worked around, not eliminated

Rank frames by per-stage divergence (weighted toward the stages the predicted-floor comparison flags
as exceeding budget) and surface a short, prioritized list of the worst offenders -- seconds of audio,
not a full corpus -- for a real human listening check at the milestones that matter. This doesn't
remove the need for a human ear; it makes the ask tractable (a handful of specific, likely-worst-case
clips) instead of exhaustive (listen to everything, or trust an unvalidated proxy metric completely).

### Becomes a real regression suite once validated

Once a first pass gets real human sign-off on a chosen set of per-stage divergence bounds, those
bounds become ordinary automated test assertions -- directly matching this codebase's own standing
convention that tests exist to guard against regressions, not just document intent. Future tuning
changes (a word-width change, a different rounding mode, a new stage's fixed-point implementation)
get an automatic, objective "did this make anything worse" signal without requiring a fresh human
listen every time -- only when an automated bound is newly violated does it need one.

## Real measured ranges, 2026-09-04 -- the validation harness's own first step, actually run

Per the "Validation harness design" section's own first honest step above: instrumented the real M17
float reference directly (temporary `INSTR_*` macros added to `lpc.c`/`nlp.c`, compiled in only via
`-DCODEC2_INSTRUMENT`, not part of the real Codec2-mod source) and ran it against 50.8 real seconds
of real speech -- five different speakers from `rade_c/wav/`, downsampled from their native 16kHz to
Codec2's own required 8kHz via `scipy.signal.resample_poly` (a standard, legitimate way to get real
8kHz test material from real recordings, not synthetic). Real results, corrected against the
back-of-envelope table above where they differ:

| Quantity | This proposal's own earlier estimate | Real measured (2539 real frames, 5 speakers) |
|---|---|---|
| `R[0]` (windowed-signal energy) | "~10^6-10^7 range for a full-scale frame" | **min 4.15x10^-6, max 8.60x10^5** |
| Levinson-Durbin min `\|e\|` | "can shrink toward zero for resonant/nasal frames" | **min 2.67x10^-6**, at the *same* frame as `R[0]`'s own minimum |
| `lpc_to_lsp` `P[]`/`Q[]` max `\|coeff\|` | "roughly single-to-low-double digits" | **77.2** |
| `aks_to_mag2` `A2[]` range | "bounded... can approach its `1e-6f` floor" | **min 4.08x10^-6, max 6167** |
| `aks_to_mag2` max `invA2` | "floor-bounded max (~10^6)" | **244,880** (about 1/4 of the theoretical ceiling) |
| `nlp()` max post-square power | "on the order of 10^20-10^23" | **2.30x10^19** -- same order of magnitude, real measurement lands at the conservative end of the earlier estimate |

**One real correction to this proposal's own earlier reasoning, not just number-filling**: the
"LPC-analysis path is much tamer" asymmetry claimed above is true for the *maximum* (confirmed: real
max `R[0]` is ~8.6x10^5, nowhere near NLP's own ~10^19 path) but **wrong about the overall dynamic
range being fixed-point-friendly** -- real speech includes real quiet passages and pauses between
words, and `R[0]` measured a genuine ~10^11 span (4x10^-6 to 8.6x10^5) across real content, not just a
bounded-above value. A linear Q-format sized to hold the real maximum without overflow has
essentially zero effective precision at the real minimum, 11 orders of magnitude below it -- **`R[0]`
and `A2[]` need the same block-floating-point/log-domain treatment already recommended for the NLP
power path and the Levinson-Durbin error term, not the "fits a 32-bit accumulator with real headroom"
treatment the table above suggested.** The real, measured driver of Levinson-Durbin's own near-zero
`e` is genuinely quiet audio specifically (confirmed: the minimum `\|e\|` occurred at exactly the same
frame as the minimum `R[0]`), not resonant/nasal spectral content as originally guessed -- a real,
more common, and more load-bearing risk case than the original framing implied, since every real
recording has quiet passages, while strongly resonant/nasal content is comparatively rarer.

The `P[]`/`Q[]` polynomial-coefficient real maximum (77.2) is real, useful, and more precise than the
"single-to-low-double-digit" guess -- a fixed-point format for this stage needs at least 7 integer
bits of headroom for the coefficient magnitude alone, not the smaller margin the earlier guess would
have suggested.

**Instrumentation and measurement corpus not committed anywhere** -- this was a real, run, one-time
measurement pass (temporary source patches applied to a scratch copy of the cloned M17 repository, a
small custom C harness, and downsampled copies of already-existing `rade_c/wav/` speech), not new
code landing in this repository. A real next step for whoever picks this up: re-run across a larger,
more deliberately adversarial corpus (very quiet recordings specifically, not just typical speech)
before finalizing any specific Q-format, since this pass's own 50.8-second sample, while real, is not
exhaustive -- the true minimum `R[0]`/`e` a real deployment could see is very plausibly lower still on
a genuinely silent frame (this sample's speakers were talking throughout, with only ordinary
inter-word pauses, not extended silence).

## A real, significant risk found by actually testing Levinson-Durbin against real data, 2026-09-04

This proposal's own per-stage table already flagged Levinson-Durbin as "the highest numerical risk
in the whole codec" because of its shrinking error term. Testing that claim against the real R[]
vectors captured above (2532 real vectors, `codec2_r_dump.txt`) found something more specific and more
serious than plain precision loss: **the risk is a genuine bifurcation at the code's own `if
(fabsf(k) > 1.0f) k = 0.0f;` safety clamp, not a shrinking-precision problem more mantissa bits can
fix.**

**What was tested**: a block-floating simulation (`e` and `k` explicitly requantized to a chosen
mantissa width every iteration, standing in for a real fixed-point representation before committing
to exact bit-widths) run against every real captured `R[]` vector, comparing final LPC coefficients
against the exact float reference (`lpc.c`'s own `levinson_durbin`, transcribed byte-for-byte).

**Real result: mantissa width made no measurable difference.** 16-bit, 20-bit, 24-bit, 28-bit, and
32-bit mantissas for `e` all produced the *identical* divergence count -- 24 of 2532 real frames
(0.9%) landed more than 1.0 away from the float reference in at least one LPC coefficient, with a
worst-case error of 14.29 on a coefficient whose real correct value was 14.29 (i.e. the fixed-point
result was completely wrong, not just imprecise). **Traced to a specific mechanism, not left as an
unexplained number**: on the worst real frame found, the float reference computes a reflection
coefficient of 0.722 at iteration 4 (comfortably inside the valid range) while the quantized path's
tiny accumulated rounding difference from the *previous* three iterations pushes its own iteration-4
computation to 1.094 -- just over the clamp threshold, forcing `k=0` where the reference used a real,
non-zero value. That one difference cascades: every remaining iteration now recurses from the wrong
`a_prev[]` state, and the final coefficients diverge by an order of magnitude, not a rounding-sized
amount.

**Confirmed this is a real property of the algorithm itself, not an artifact of the simulation**: the
same test, with no fixed-point or quantization involved at all -- just the real Codec2-mod's own
actual `float` (32-bit) precision compared against a `double` (64-bit) transcription of the identical
recursion -- still diverges on 1 of 2532 real frames (0.04%), with 10 more (0.4%) showing a real but
smaller perturbation. **Ordinary float32 already sits close enough to this same cliff edge that it
falls off it for a real, if rare, fraction of real speech**, entirely independent of any fixed-point
work. The higher 0.9% rate in the mantissa-quantization test reflects that simulation's own coarser
per-step requantization (deliberately simple, to isolate the strategy question, not tuned to match a
specific real bit-width) rather than the width itself -- but the *floor* this risk can't go below,
demonstrated by the float32-vs-float64 result, is real and nonzero regardless of how carefully a
fixed-point port is built.

**What this means for the actual port, stated plainly**: no mantissa-width choice, on its own, makes
this risk disappear -- it's inherent to computing this specific recursion along a slightly different
arithmetic path than whatever reference a test suite compares against, at the specific frames whose
own reflection coefficients sit close to the +-1 clamp boundary. Two real, honest options for whoever
builds this, neither of which this pass picked between: (a) accept a small, now-measured, real rate of
frames whose LPC estimate diverges more than expected (matching real, if imperfect, `float32` behavior
Codec2-mod already ships with, per the confirmation above), or (b) add a real, separate stabilization
step at the clamp boundary specifically -- for instance, blending `k` smoothly toward the clamp rather
than a hard snap, which changes the *reference* algorithm's own behavior slightly and would need its
own validation against real audio quality, not just numeric closeness to today's exact clamp. Neither
was attempted here -- this pass's job was finding and precisely characterizing the real risk, which a
first back-of-envelope pass correctly flagged as real but had not yet located this specifically.

### This has a real consequence for Codec2-mod's own upstream bit-exactness claim, checked directly, 2026-09-04

Codec2-mod's README states plainly: "Bit-exactness with the reference Codec2 encoder has been verified
using identical input signals and byte-for-byte comparison of encoded frames," and the fork's stated
purpose is to be "a drop-in replacement for Codec2 when using the 3200 bps mode." The clamp-boundary
bifurcation above bears directly on how strong a guarantee that actually is.

**Checked whether the fork's own refactor could be the source of the risk, not just the shared
algorithm**: cloned `drowe67/codec2` (upstream) and compared its `levinson_durbin()`
(`src/lpc.c:142-168`) against Codec2-mod's own (`src/lpc.c:32-69`) line by line. Upstream stores the
full iteration history in a 2D array (`a[order+1][order+1]`); Codec2-mod's refactor collapses that to
two rolling 1D arrays (`a[]`/`a_prev[]`) with an explicit end-of-iteration copy. But every arithmetic
operation -- `sum += a_prev[j] * R[i-j]`, `k = -(R[i]+sum)/e`, the `fabsf(k) > 1.0f` clamp, `a[i] = k`,
`e *= (1.0f - k*k)` -- reads the same operand values in the same order with the same expression shape
in both versions. **This refactor is arithmetically transparent**: on the same compiler, target, and
optimization settings, it cannot itself be the source of a bit-exactness divergence. The fork's own
verification claim is not undermined by anything introduced in the refactor itself.

**What the finding above actually shows**: the bifurcation is a property of the *shared* algorithm
(present in upstream `codec2` too, unmodified since Makhoul's original 1975 formulation this comment
block still cites), and this session's own float32-vs-float64 test proved it needs no fixed-point
involvement at all -- ordinary `float` already sits close enough to the `|k|>1` cliff to fall off it on
1/2532 real frames just from *which* mathematically-equivalent but differently-rounded arithmetic path
computed the sum. That means Codec2-mod's real, verified bit-exactness claim is corpus-and-toolchain
conditional, not universal, through no fault of the refactor: it holds for whatever input corpus and
compiler/target combination was actually used to verify it, and nothing in the algorithm guarantees it
survives a different real speech corpus, a different compiler version, a different optimization level,
or -- most relevant to Codec2-mod's own stated embedded target (the STM32F405 benchmark in its own
README) -- cross-compilation to a different architecture whose float rounding for the same source line
isn't bit-identical to whatever host compiled the reference build being compared against. None of this
is hypothetical: it is exactly the mechanism this session already reproduced with zero source changes.

**Checked GitHub for prior discussion before treating this as new**: `M17-Project/Codec2-mod` has zero
issues, open or closed, as of this check (`gh api repos/M17-Project/Codec2-mod/issues` returns `[]`) --
this has not been reported.

**Not filed upstream by this session.** Filing a GitHub issue is an action visible to a real outside
project and its maintainers, outside this codebase, so it's left as a drafted, ready-to-review writeup
at `codec2_mod_upstream_bit_exactness_note.md` (same directory) for Bruce's own judgment on whether,
and how, to raise it with the M17 project.

## The second-highest risk stage, actually tested: `aks_to_mag2`'s invA2/R/R^(2*BETA) log-domain
## treatment is well-behaved, unlike Levinson-Durbin, 2026-09-04

The per-stage table already flagged this as "second-highest risk in the codec," on the reasoning that
`invA2 = 1/A2[i]`'s huge measured dynamic range (A2[] itself: 4.08e-6 to 6167.12, so invA2: ~1.6e-4 to
244,880 -- about 9 orders of magnitude) needs log-domain handling, and noted the real float code's own
`logf`/`expf` usage in `R^(2*BETA) = expf(LPCPF_TWO_BETA * logf(R + 1e-5f))` as a strong signal from the
original authors that this specific stage already needs that treatment even in float. This pass tested
that specific proposed fix against real data, the same way Levinson-Durbin's block-floating strategy was
tested, rather than leaving it as reasoning from the code's structure alone.

**What was built**: extended the instrumented harness (`INSTR_DUMP_A2`, mirroring `INSTR_DUMP_R`/`AK`)
to capture real `(E, A2[0..255], A2g[0..255])` triples directly from `aks_to_mag2()`'s own "compute
normalization gain" loop, across the same real speech corpus used throughout this proposal --
5078 real frames (`codec2_a2_dump.txt`; 2539 encode/decode cycles x 2 sub-frames each, matching
`codec2_decode`'s own per-cycle structure). Then built a real log2/exp2-LUT implementation
(`invA2_logdomain.c`): `frexpf`-based exponent extraction (free in fixed point -- just the position of
the MSB) plus an 8-bit (256-entry) linearly-interpolated LUT for both `log2` and `exp2`, replacing the
float reference's `logf`/`expf` round-trip with `exp2_lut(LPCPF_TWO_BETA * log2_lut(R + 1e-5f))` --
mathematically the same identity (`x^k == 2^(k*log2(x))`), just computed in base 2 to match a LUT a
real fixed-point target would actually implement, matching the ITU G.729-style "Log2"/"Pow2" basic-
operator precedent this proposal already cites.

**Real result: no bifurcation, clean smooth convergence.** Across all 5078 real frames, the log2/exp2-
LUT candidate's `gain` value differs from the exact float reference by a **max relative error of
8.25e-7, RMS 3.64e-7** -- within a handful of float32 ULPs (float32 epsilon is ~1.19e-7), with no
outlier frames and no catastrophic divergence anywhere in the corpus. **Confirmed this reflects real
LUT resolution, not a vacuous test**: rerunning with a deliberately coarse 4-bit (16-entry) LUT instead
of 8-bit pushed the max relative error up to 1.59e-4 (RMS 8.73e-5) -- about 200x worse, exactly the
kind of resolution-dependent degradation a real approximation should show, confirming the 8-bit result
is a genuine, working approximation and not an artifact of the test comparing float against itself.

**This is a materially different risk shape than Levinson-Durbin's.** Levinson-Durbin's risk is a
discontinuity (a hard clamp boundary that either fires or doesn't, with no middle ground, producing
order-of-magnitude errors on a small but real fraction of frames regardless of precision). The
log2/exp2-LUT treatment of `invA2`/`R`/`R^(2*BETA)` has no such cliff: every operation involved
(`frexpf`'s exponent/mantissa split, LUT lookup, linear interpolation, `ldexpf`'s power-of-2 scaling)
is smooth and monotonic in its own domain, so approximation error degrades gracefully and predictably
with LUT resolution rather than snapping between two qualitatively different outcomes. **This specific
proposed treatment for the second-highest-risk stage is validated against real data and ready to build
on** -- unlike Levinson-Durbin, this stage does not need a stabilization-vs-accept decision; an 8-bit
LUT is already accurate enough that the remaining question is purely an engineering one (exact Q-format
widths for the surrounding fixed-point multiply/divide/sqrt operations this test deliberately left in
float to isolate the log-domain piece specifically), not an open numerical-soundness question.

## First real fixed-point primitive, built and validated, 2026-09-04

The per-stage table's own claim that `phase_synth_zero_order()`'s phase-accumulator pattern is "the
easiest stage in the whole codec... extremely well precedented in fixed point" was checked by
actually building it, in C (matching the real target language, not this project's own Rust, since
Codec2-mod itself is C and the whole point is running on FPU-less embedded targets Rust's own
ecosystem doesn't reach the same way): a 32-bit phase accumulator (0 to `UINT32_MAX` mapped to `[0,
2*pi)`, the standard hardware DDS/NCO representation) plus a 1024-entry quarter-wave Q15 `sin` LUT
with linear interpolation, exploiting the real four-fold symmetry to also produce `cos` from the same
table.

**The real design choice this validates, not just "a LUT exists"**: representing phase as a fixed-
width binary fraction of one full circle means wraparound is *free* -- ordinary unsigned-integer
overflow, not an explicit `fmod`/modulo step anywhere in the per-sample hot path. This matters
specifically for the real code's own `codec2_sincosf(phi0 * m, ...)` call (`synthesis.c`), where `m`
(the harmonic number) runs up to `MAX_AMP` = 80 and the resulting unwrapped angle can reach roughly
`80*pi` -- real libm `sincosf` handles that via its own internal range reduction; this fixed-point
design gets equivalent correctness for free from the representation itself (`phase * m` still just
wraps correctly via the same unsigned overflow), not from an added reduction step that would need its
own separate validation.

**Real, measured validation, not just "it compiled"**: two tests, against real `double` sin/cos as
ground truth --

1. A full-circle sweep (100,000 points): max absolute error 4.52x10^-5, RMS error 1.92x10^-5.
2. **The actual `phase_synth_zero_order()` case**: a real mid-range `Wo` (pitch period 45 samples)
   accumulated across 1000 real 20ms frames, checked at every harmonic 1 through 80 each frame
   (80,000 total checks) -- max absolute error 3.73x10^-5, RMS error 1.91x10^-5, **the same order of
   magnitude as the base LUT sweep, not degraded by the per-harmonic multiplication or by 1000 frames
   of accumulated phase**. The per-harmonic-multiplication concern this design specifically had to get
   right shows no measurable extra error versus the base case.

**A genuinely useful, unplanned finding**: this error is already at Q15's own output-quantization
floor (1/32768 ~= 3.05x10^-5) -- the 1024-entry LUT is not the limiting factor at all, meaning it
could likely be shrunk (fewer entries, less ROM) without losing real precision, a real optimization
opportunity for an actual embedded target that wasn't the goal of this pass but fell out of measuring
honestly rather than assuming the chosen LUT size was already optimal.

**Not done in this pass**: this is one validated primitive (the phase accumulator + sin/cos LUT), not
a wired-in replacement for `codec2_sincosf` inside a real, running fixed-point `phase_synth_zero_order`
-- that integration, and the `Ex[m]`/`A_[m]` complex multiply surrounding it in the real function, are
real, separate, smaller follow-on steps. The code itself (`phase_dds.c`) lived in this session's own
scratchpad, not committed to any repository -- reproducible directly from this section's own
description (32-bit NCO phase representation, 1024-entry Q15 quarter-wave LUT, linear interpolation,
quadrant symmetry for cos) for whoever picks this up next.

### Correction: the synthetic constant-Wo test above understated the real error by ~9x, checked
### against real captured pitch trajectories, 2026-09-04

The "not degraded" claim above was checked against real data, not left resting on the synthetic
constant-`Wo` test. Extended the instrumented harness (`INSTR_DUMP_WO`) to capture every real
`(Wo, L, voiced)` sub-frame actually produced decoding the real speech corpus (5078 sub-frames: 3516
voiced, 1562 unvoiced), then re-ran the identical `fixed_sincos` primitive against that real trajectory
-- accumulating `ex_phase[0]` exactly as `phase_synth_zero_order()` does (every sub-frame,
voiced or not, matching the real code), and checking sin/cos at harmonics 1 through `L` on voiced
sub-frames only (real code notes phase isn't perceptually needed for unvoiced sound, and the unvoiced
branch's PRNG-derived phase isn't reproducible bit-for-bit by this standalone replay).

**Real result: max absolute error 3.47x10^-4, RMS 5.18x10^-5, across 145,576 real checks -- about 9x
worse worst-case than the synthetic sweep's 3.73x10^-5.** Traced, not left unexplained: the worst case
was `Wo=0.0393` (a long pitch period, near `P_MAX`) at harmonic `m=79` (near `MAX_AMP=80`) -- exactly
where the mechanism this design already flagged as needing scrutiny (`phase_m = phase_acc * m`) is
expected to matter most: any fixed-point quantization noise already present in `phase_acc` itself gets
multiplied by `m` before the LUT lookup, so error at high harmonics of a low-`Wo` (many-harmonic)
voiced frame scales with `m`, not with the LUT's own flat per-lookup floor the synthetic test happened
to sit at. The synthetic test's single constant `Wo` (pitch period 45) never exercised the
low-`Wo`/high-`L` combination real speech's own low-pitched voiced frames actually produce.

**Still small in absolute terms** (3.47x10^-4 is well below, e.g., a typical 8-bit-or-coarser
quantizer's own step size), but this is a real, measured correction to the earlier claim, not a
reassuring restatement of it: **"not degraded by the per-harmonic multiplication" was wrong** for the
real range of `(Wo, m)` combinations real speech produces, even though the base LUT/wraparound design
itself remains sound and the absolute error stays small. Whoever wires this primitive in for real
should re-check the worst-case error at this stage's own actual chosen LUT resolution and Q-format,
not assume the synthetic-sweep numbers above characterize the real worst case.

## What this proposal does not attempt

No fixed-point code was written in this pass -- per Bruce's own framing ("make a proposal"), this is
the domain characterization and plan, not the implementation. Real, honest open items before
implementation could start:

- The empirical bounds given above for `R[0..10]`, LPC/LSP polynomial coefficients, and `A2`/`invA2`
  are reasoned from the code's own structure and representative worst-case inputs, not yet measured
  across a real speech corpus the way this project's own DSP research (`SSB_SPECTRAL_TUNING_RESEARCH.md`,
  `PSK31_SCANBANK_ARBITRATION_RESEARCH.md`) insists on before trusting a number. The "Validation
  harness design" section above is the plan for closing this -- instrumenting the real M17 float
  reference across a real speech corpus (LibriSpeech, which `delta_lsp_cb.c`'s own comments confirm
  the M17 project already used for codebook design) to confirm or correct every bound in the table
  above before committing to specific Q-formats. **The harness itself is design only in this pass, not
  built** -- no logging code, no corpus run, no actual divergence numbers yet.
- No decision is made here about target hardware, word width, or whether to fork Codec2-mod directly
  or write a from-scratch fixed-point implementation guided by it -- both are real options this
  proposal deliberately leaves open for Bruce's own call.
- Whether M17's own protocol requires (or would benefit from) a fixed-point Codec2-mod for real
  interoperability reasons, versus this being purely this project's own exploratory interest, wasn't
  established -- worth checking with the M17 project directly (e.g. their own GitHub issues/mailing
  list) before investing further, since bit-exact reproducibility (this proposal's own strongest
  motivation, per "Why fixed-point at all" above) only matters if some other real implementation is
  actually going to need to interoperate bit-exactly with this one.
