# Fixed-Point Encoder Implementation: Punch List

## Status: scoped 2026-09-04, per Bruce's direct request "Implement the fixed-point encoder." Real
## structure now exists (`floating_reference::Encoder` + a parallel `EncoderFixed`, both build and
## pass their own round-trip test), and `is_voiced_fixed` is genuinely wired into `EncoderFixed`. The
## `autocorrelate` gate initially turned out harder than scoped (a fixed-point r0-normalization
## candidate failed on frame 273 for the same structural reason `div_round_i128`'s reciprocal-multiply
## idea was already rejected) -- **then resolved at the root**: `lpc::apply_white_noise_correction`
## (a standard technique, Rabiner & Schafer 1978) fixes Levinson-Durbin's own numerical fragility
## directly, and independently resolved the r0-normalization candidate's failure too (0 unexplained
## divergences, not just fewer) -- real, direct evidence it's a root-cause fix, not a narrow patch.
## Wired into both real encoders already. Sent to the codec2 mailing list. The r0-normalization
## candidate itself is still not wired into any real accumulator -- see that row. Everything else is
## real, unstarted, multi-session work. This file is the ground truth for what's actually done --
## re-derive from here against the real code, not from a chat summary, before ever reporting this
## "complete."

## White noise correction -- a root-cause fix, not part of the original punch list, added 2026-09-05

Not one of the stages below -- a fix to the underlying numerical fragility several of them run into.
`lpc::apply_white_noise_correction` multiplies `R[0]` alone by `(1 + 1e-3)` before Levinson-Durbin
(leaving `R[1..LPC_ORD]` untouched -- see that function's own extensive doc comment, written with
David Rowe as the intended external reader, for the full derivation). Real, measured effect against
the real `codec2_r_dump.txt` corpus (362 frames): worst-case `1/e` amplification drops from ~3581x to
~269x, a >13x improvement, for an energy correction (~30dB below signal) far under the audible
threshold. **Confirmed to independently resolve the `autocorrelate` row's own r0-normalization
failure** (below) -- real evidence this addresses a shared root cause, not one narrow symptom.
Wired into both `floating_reference::Encoder` and `EncoderFixed` (a separate copy of `R[]` feeds only
Levinson-Durbin; `lpc_energy` still sees the real, uncorrected energy). **Real, honest consequence**:
this changes `floating_reference::Encoder`'s actual output, so the specific real cross-decoder interop
numbers documented in `mod.rs`'s own module doc comment predate this change and need re-measuring (a
manual step outside this crate's automated build) before being trusted as current again.

**Real follow-up questions asked and answered, not yet acted on further**:
- *Does this unlock `i64`-to-`i32` narrowing for Levinson-Durbin's own Q8.40 format?* Only modestly --
  the bits-needed-vs-amplification relationship is logarithmic, so a 13.3x amplification reduction
  only recovers `log2(13.3) ~ 3.7` bits of headroom, nowhere near the ~17-bit gap between Q8.40 and
  the Q8.23 format that was already measured insufficient. Not enough alone to reach `i32`.
- *Does this obsolete the small-`e` branch-to-wide-path design already recorded in
  `CODEC2_FIXED_POINT_WIDTH_REDUCTION_STUDY.md`?* No -- complementary, and more attractive now: the
  correction reduces both how often a real frame would trigger the wide path and how extreme that
  path needs to be for the frames that still do, but doesn't eliminate the need for it on the
  remaining, now-rarer, ill-conditioned frames.

## Structure, resolved 2026-09-04 -- Bruce's own call on the open product decision below

Parallel, not in-place replacement: `Encoder` moved to `codec2_3200::floating_reference::Encoder`
(fully `f32`, untouched otherwise, kept live specifically as the per-frame diff reference every
fixed-point stage gets checked against) and `codec2_3200::EncoderFixed` (`encoder_fixed.rs`) is the
new, real, forward-looking build -- both compile, both pass a real round-trip sanity test today.
`EncoderFixed`'s own doc comment is explicit that most of its pipeline still delegates to the same
`f32` functions `floating_reference::Encoder` calls (converting its own `i16`-native `sn` to `f32` at
each not-yet-migrated stage's own call site) -- only `voicing::is_voiced_fixed` is genuinely
fixed-point in `EncoderFixed` today. `Decoder` did not move; see `floating_reference/mod.rs`'s own
doc comment for why encode/decode aren't symmetric here.

## Why this is an architecture change, not an integration task

`CODEC2_FIXED_POINT_WIDTH_REDUCTION_STUDY.md` and `CODEC2_MOD_FIXED_POINT_PLAN.md` both validated
individual arithmetic *stages* against a plain-`f32` reference, deliberately converting back to
`f32` at each stage's own output boundary ("integer core, float boundary" -- `levinson_durbin_fixed`
itself returns `LpcCoeffs` = `[f32; 11]`, not a fixed-point type). That pattern was the right call
for validating one stage at a time, but it means **every stage boundary in `Encoder::encode()` is
`f32` by deliberate design, not by neglect** -- `Autocorr = [f32; 11]`, `LpcCoeffs = [f32; 11]`, and
so on. A real fixed-point encoder means redesigning those boundaries to carry a fixed-point (or
block-floating) representation across stages, not just wiring in the two already-validated
components (`levinson_durbin_fixed`, `cheb_poly_eval_fixed`) as-is -- wiring them in today would
still round-trip through `f32` at every boundary, which isn't what "fixed-point encoder" means.

## Punch list -- stage, current state, real acceptance bar, done or not

| Stage | Current state | Real acceptance bar | Status |
|---|---|---|---|
| `window.rs` (`make_analysis_window`) | Computed at runtime in `f32`, but the window shape is a compile-time constant | Quantize the constructed window to a fixed-point output type with real measured margin | **Candidate built and validated, NOT WIRED.** `make_analysis_window_fixed() -> [i32; M_PITCH]` added, Q0.30 (real measured peak ~0.0043, real margin under `i32`), validated against the real `f32` construction (max dequantized error < 1e-8). Not used by `EncoderFixed` yet -- nothing downstream (`autocorrelate`) is ready to consume a fixed-point windowed sample, so wiring this in now would just add a pointless round trip. |
| `voicing.rs` (`is_voiced`) | Corrected after starting: `energy_db = 10*log10(energy)` is a real dependency on the still-incomplete `log2_lut` (not just a threshold to narrow) | See resolution below | **DONE and WIRED into `EncoderFixed`** -- the one genuinely fixed-point stage in the real forward-looking encoder today. Real design change, not a mechanical port -- see below. |
| `autocorrelate` + `Autocorr` boundary | Full `f32`. `levinson_durbin_fixed_core`'s very first line already divides every `R[j]` by `r0` (in `f32`) -- cancelling the "shared exponent" a block-floating design would have tracked, so that framing was more machinery than the real problem needs. A fixed-point `r0`-normalization candidate (raw `R[]` in a single wide Q20.43 `i64` format -- 20 integer bits for real max `R[0]` ~8.6e5, 43 fractional for real margin at the real min ~4e-6, both under `i64`'s 63 usable bits, confirming block-floating genuinely isn't needed) *originally failed* the existing discriminator methodology on frame 273 (this corpus's own known worst-conditioned frame), for the same structural reason `div_round_i128`'s reciprocal-multiply idea was already rejected: even a *more* mathematically exact division than the current `f32` one diverged, because frame 273 was fragile enough that ordinary `f32` rounding was load-bearing for the current implementation's own passing status. **Resolved, not worked around**: applying `apply_white_noise_correction` (see the section above) before this candidate's normalization runs closes the gap completely -- 0 unexplained divergences across the whole corpus, confirmed by the test itself (`r0_normalization_fixed_point_candidate_diverges_from_float_only_at_measured_clamp_disagreement_frames`, now passing at the real `== 0` bar, not a loosened one). | Real bit budget confirmed sufficient (Q20.43 fits `i64` with margin), block-floating confirmed unnecessary, and the normalization candidate itself now has a real, passing acceptance test. What's left: (1) wire this candidate's normalization into a real function (it currently lives only inside a test module); (2) still needs the real input-side bit budget for `autocorrelate`'s own accumulator (`wn[i]` = `i16` sample x Q0.30 window coefficient, summed over 320 terms) -- unmeasured; (3) then wire `levinson_durbin_fixed` itself into `EncoderFixed` using this real, integer `R[]`/normalization path end to end. | **Real path forward exists and is validated; not yet wired into a real (non-test) function.** The hard part (a sound, passing acceptance test for the fixed-point normalization) is done. What remains is real but mechanical: promote the candidate out of the test module, build the accumulator, wire it in. |
| `levinson_durbin_fixed` wiring | Built and validated (`levinson_durbin_fixed_diverges_from_float_only_at_measured_clamp_disagreement_frames`), but not called by `Encoder::encode()`, and its own output boundary is `f32` | Redesign its output to carry a fixed-point/block-floating type forward (not `LpcCoeffs`/`f32`), then wire into `Encoder::encode()` in place of `levinson_durbin` | **NOT STARTED**, blocked on the `autocorrelate` boundary above |
| `lpc_energy` | Full `f32` | Fixed-point port + real fixture-corpus validation | **NOT STARTED** |
| `bw_gamma` (bandwidth expansion) | Full `f32`, but `lpc.rs`'s own doc comments already note this "turned out to reduce to simple formulas" elsewhere in this codebase's history -- may be more tractable than it looks | Fixed-point port + validation | **NOT STARTED** |
| `build_p_q` | Full `f32` | Fixed-point port + validation | **NOT STARTED** |
| `cheb_poly_eval_fixed` / `find_next_root` / `lpc_to_lsp` | **Already fixed-point internally** (this session: `i32`, proven bit-exact, real codegen win) | Its own input (`ak`) still arrives as `f32` from `build_p_q` -- not closed until `build_p_q` is | **PARTIALLY DONE** -- internal arithmetic done, input boundary still float |
| `fixed_point.rs`'s `log2_lut`/`exp2_lut` | Real LUT-based approach exists, but the interpolation step itself is genuine `f32` (that file's own doc comment, flagged three times this session already) | Fixed-point interpolation, validated against the real encoder-side data this file's own existing tests already use | **NOT STARTED** -- on the real encode path via `quantise::encode_energy`, not optional |
| `quantise.rs` (`encode_wo`, `encode_lsps_delta_scalar`) | Full `f32` | Fixed-point port + validation | **NOT STARTED** |
| `nlp.rs` (FFT-based pitch estimator) | Full `f32`, uses `rustfft`/`Complex32` | **Different, weaker bar than everything above**: `nlp.rs`'s own module doc already establishes this has full design freedom -- a real decoder only ever sees the quantized `Wo` index, never this module's internal arithmetic, so the bar is "finds the same fundamental," not "reproduces the reference's arithmetic." Needs a fixed-point radix-2 FFT (`PE_FFT_SIZE` is fixed, input is real-valued -- a known design, not research) with a static twiddle table and real per-stage overflow/scaling analysis, plus its own validation corpus. | **NOT STARTED -- scope this as its own separate pass**, not bundled with the LPC-chain work above. Substantially larger than any single item above. |
| `bits.rs` (`pack_frame`) | Already integer/bit-packing | N/A -- already done, not a gap | **DONE** (pre-existing, not part of this pass) |

## This pass: window.rs and voicing.rs

Both landed with real validation, described in their own commits. Chosen first because they're
independent of the `autocorrelate` gate and of each other -- matches this session's own established
"don't block independent work on a sequencing preference" discipline.

**voicing.rs's real correction, worth recording explicitly**: this table originally scoped it as
"threshold narrowing" (advisor's own initial framing, before implementation surfaced the real
dependency). `is_voiced`'s `energy_db > noise_floor_db + MARGIN_DB` check needs a real `log10`, which
routes through `fixed_point.rs`'s `log2_lut` -- itself flagged elsewhere in this table as having a
genuine `f32` interpolation gap. Rather than build on that incomplete piece, `is_voiced_fixed`
sidesteps the log entirely: `energy_db > noise_floor_db + MARGIN_DB` is mathematically equivalent to
a **linear**-domain `energy > noise_floor_energy * 10^(MARGIN_DB/10)`, so the comparison needs only
one precomputed constant ratio, no runtime log or pow. This is a real, deliberate algorithmic
difference (an EMA over linear energy is not the same as an EMA over dB -- geometric- vs.
arithmetic-flavored averaging), permissible per this module's own documented design freedom ("doesn't
need to reproduce [the reference]'s exact decision, only make a reasonable one"), and validated by
running `is_voiced_fixed` against the *same* four real scenarios `is_voiced` itself is validated
against (clean tone -> voiced, white noise -> not voiced, silence -> not voiced, quiet tone below a
settled noise floor -> not voiced) -- all four matched. **Wired into `EncoderFixed`** (which stores
raw `i16` samples natively, unlike `floating_reference::Encoder`'s `f32` `sn` -- this is the reason
`EncoderFixed`'s own `sn` field is `i16`, not just an aesthetic choice).

## Explicitly not attempted this pass

Everything marked NOT STARTED above. In particular: the `autocorrelate`/block-floating boundary
(the real gate on the rest of the LPC chain) and the fixed-point FFT (the largest single remaining
item) are both real, substantial, multi-session pieces of work, not started here. Do not report this
punch list as closed without re-reading this table against the actual current code -- per this
project's own standing "keep-working-until-actually-done" discipline, a status header can go stale;
re-derive from `grep`ing the real function signatures, not from this file's own prose, before
trusting it.
