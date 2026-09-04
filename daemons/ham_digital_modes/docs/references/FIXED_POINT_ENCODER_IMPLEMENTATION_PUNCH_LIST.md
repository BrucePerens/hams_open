# Fixed-Point Encoder Implementation: Punch List

## Status: scoped 2026-09-04, per Bruce's direct request "Implement the fixed-point encoder." Real,
## substantial progress: `EncoderFixed`'s entire windowing -> `autocorrelate` -> Levinson-Durbin ->
## `lpc_energy` -> bandwidth-expansion chain is now genuinely fixed-point end to end, no `f32`
## anywhere in it -- the LPC coefficient and energy estimate, arguably the most numerically fragile
## part of this whole codec, no longer touch float in `EncoderFixed`. Validated not just in isolation
## but by direct comparison against `floating_reference::Encoder` on identical input: decoded-audio
## correlation **1.0**. Getting there required a real detour: a fixed-point `r0`-normalization
## candidate initially failed on frame 273 (the same structural reason `div_round_i128`'s
## reciprocal-multiply idea was already rejected), which led to `lpc::apply_white_noise_correction` (a
## standard technique, Rabiner & Schafer 1978) -- a real root-cause fix for Levinson-Durbin's own
## fragility, sent to the codec2 mailing list, which independently resolved the normalization
## candidate's failure too. `lpc_to_lsp`'s own input boundary (`build_p_q`), the LSP/energy/Wo
## quantizers, and the pitch estimator (`nlp.rs`) are still `f32` and not yet started. This file is
## the ground truth for what's actually done -- re-derive from here against the real code, not from a
## chat summary, before ever reporting this "complete."

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
| `window.rs` (`make_analysis_window`) | Was: computed at runtime in `f32`. Now: `window::make_analysis_window_fixed() -> [i32; M_PITCH]` (Q0.30) is a real field on `EncoderFixed`, feeding the real windowing multiply below. | Quantize with real measured margin, feed a real fixed-point consumer | **DONE and WIRED into `EncoderFixed`.** |
| `voicing.rs` (`is_voiced`) | Corrected after starting: `energy_db = 10*log10(energy)` is a real dependency on the still-incomplete `log2_lut` (not just a threshold to narrow) | See resolution below | **DONE and WIRED into `EncoderFixed`**. Real design change, not a mechanical port -- see below. |
| `autocorrelate` + `Autocorr` boundary | Was: full `f32`. Now: `lpc::autocorrelate_fixed(wn_q: &[i32]) -> [i64; LPC_ORD+1]`, real, wired into `EncoderFixed`. `wn_q` is Q8.23 (real measured peak ~141.8, matches `COEF_FRAC_BITS`'s own established convention); each per-term product widens to `i64` transiently then shifts right immediately, so the running sum never needs more than ordinary `i64` headroom (real measured worst case ~5.4e13, `i64::MAX` ~9.2e18 -- over 17 bits of margin). Getting here required a real detour: a candidate `r0`-normalization *originally failed* frame 273 (this corpus's own known worst-conditioned frame) for the same structural reason `div_round_i128`'s reciprocal-multiply idea was already rejected -- even a division *more* exact than the current `f32` one diverged, because frame 273 was fragile enough that ordinary `f32` rounding was load-bearing. **Resolved, not worked around**: `apply_white_noise_correction` (see the section above) closes the gap completely, confirmed twice -- once at a synthetic wide Q(43) precision (isolating the normalization question), once again at `autocorrelate_fixed`'s own real, narrower Q8.23 output (the actual end-to-end precision) -- 0 unexplained divergences both times. | Real integer accumulator, real bit budget confirmed (both the input side, validated against `synthetic_codec2_wn_dump.txt`/`synthetic_codec2_r_dump.txt`, max relative error 1.37e-5; and the normalization, validated against `codec2_r_dump.txt`), wired into a real consumer | **DONE and WIRED into `EncoderFixed`.** |
| `levinson_durbin_fixed` wiring | Was: built and validated but not called by any real `Encoder`, output boundary `f32`. Now: `levinson_durbin_fixed_core` refactored to extract `levinson_durbin_fixed_core_from_r_norm` as the real shared recursion body (used by both the original `f32`-normalizing entry point and the new one); `r0_normalize_fixed` and `levinson_durbin_fixed_from_integer_r` promoted from test-only candidates to real production functions, replacing the test module's own now-redundant local duplicates. Output stays `LpcCoeffs`/`f32` deliberately (see that function's own doc comment) -- downstream (`build_p_q`) isn't migrated, so there's nothing further to hand an integer type to yet. | Wire into a real encoder, validate against the float reference on real input, not just the isolated fixture corpus | **DONE and WIRED into `EncoderFixed`.** Real, direct comparison against `floating_reference::Encoder` on identical synthetic input: decoded-audio correlation **1.0**. |
| `lpc_energy` | Was: full `f32`, forcing `EncoderFixed` to run a real, separate duplicate float windowing/`autocorrelate` pass just to feed it. Now: `lpc::lpc_energy_fixed(a_q23, r_q) -> f32` (each Q8.23 per-term product widens to `i128` transiently, shifts back to Q8.23, accumulates in `i64`), wired into `EncoderFixed`, duplicate pass removed. | Real fixture-corpus validation, checked against the bar that actually matters (the transmitted quantizer index, not raw relative error -- see below) | **DONE and WIRED into `EncoderFixed`.** 0/362 real quantizer-index mismatches vs `lpc_energy` on real captured `ak`/`R[]` data (`quantise::encode_energy` agreement, not a raw-value tolerance -- an initial raw-relative-error version found a real, harmless 3.2% worst case from genuine cancellation in `sum(ak[i]*R[i])` on some frames, ~0.14dB, over 11x smaller than the 5-bit quantizer's own 1.5625dB step). |
| `bw_gamma` (bandwidth expansion) | Was: full `f32`. Now: `lpc::apply_bw_gamma_fixed`, a real literal Q8.23 table (`BW_GAMMA_Q23`, generated from `bw_gamma`'s own real output, not an independently-computed `0.994^i`), mutating `a_q23` in place. | Fixed-point port + validation against real captured `ak[]` data | **DONE and WIRED into `EncoderFixed`.** Real absolute-error validation (3.6e-7, ordinary Q8.23 noise) -- an initial relative-error version found a real, harmless 0.043% worst case from small-denominator amplification, the same class of finding as `lpc_energy_fixed`'s own cancellation case; switched to the honest metric (absolute error) for a plain elementwise scale. |
| `build_p_q` | Full `f32` | Fixed-point port + validation | **NOT STARTED** |
| `cheb_poly_eval_fixed` / `find_next_root` / `lpc_to_lsp` | **Already fixed-point internally** (this session: `i32`, proven bit-exact, real codegen win) | Its own input (`ak`) still arrives as `f32` from `build_p_q` -- not closed until `build_p_q` is | **PARTIALLY DONE** -- internal arithmetic done, input boundary still float |
| `fixed_point.rs`'s `log2_lut`/`exp2_lut` | Real LUT-based approach exists, but the interpolation step itself is genuine `f32` (that file's own doc comment, flagged three times this session already) | Fixed-point interpolation, validated against the real encoder-side data this file's own existing tests already use | **NOT STARTED** -- on the real encode path via `quantise::encode_energy`, not optional |
| `quantise.rs` (`encode_wo`, `encode_lsps_delta_scalar`) | Full `f32` | Fixed-point port + validation | **NOT STARTED** |
| `nlp.rs` (FFT-based pitch estimator) | Full `f32`, uses `rustfft`/`Complex32` | **Different, weaker bar than everything above**: `nlp.rs`'s own module doc already establishes this has full design freedom -- a real decoder only ever sees the quantized `Wo` index, never this module's internal arithmetic, so the bar is "finds the same fundamental," not "reproduces the reference's arithmetic." Needs a fixed-point radix-2 FFT (`PE_FFT_SIZE` is fixed, input is real-valued -- a known design, not research) with a static twiddle table and real per-stage overflow/scaling analysis, plus its own validation corpus. | **NOT STARTED -- scope this as its own separate pass**, not bundled with the LPC-chain work above. Substantially larger than any single item above. |
| `bits.rs` (`pack_frame`) | Already integer/bit-packing | N/A -- already done, not a gap | **DONE** (pre-existing, not part of this pass) |

## This pass: window.rs, voicing.rs, then the whole autocorrelate/Levinson-Durbin chain

window.rs and voicing.rs landed first, independent of the `autocorrelate` gate and of each other --
matches this session's own established "don't block independent work on a sequencing preference"
discipline. `autocorrelate`/Levinson-Durbin followed once white noise correction resolved the real
blocker found along the way; see the dedicated section above and each stage's own row for the full
detail, and each real commit for the exact sequence (the `r0`-normalization candidate's own failure
and resolution, the real integer accumulator, the end-to-end wiring and comparison against
`floating_reference::Encoder`).

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

## Real follow-on, raised by Bruce, not yet started: move reference-only float code into `floating_reference/`

Currently only `Encoder` itself lives in `floating_reference/mod.rs` -- the actual float
*implementations* it calls (`lpc::autocorrelate`, `lpc::levinson_durbin`, `lpc::lpc_energy`,
`lpc::build_p_q`, `lpc::lpc_to_lsp`, `bw_gamma`, `voicing::is_voiced`, `window::make_analysis_window`,
`nlp::nlp`) still live in their original shared modules. As of `build_p_q`/`lpc_to_lsp` migrating
(this pass), `bw_gamma`, `autocorrelate`, `levinson_durbin`, `lpc_energy`, `build_p_q`, `lpc_to_lsp`,
and `voicing::is_voiced` are all genuinely exclusive to `floating_reference::Encoder` now -- no
`EncoderFixed` call site needs them anymore -- and could reasonably move. **Two real, named
exceptions that can't move without breaking the fixed path**: `window::make_analysis_window` (the
`f32` version) is still called *inside* `make_analysis_window_fixed` itself to derive its own
quantized table; `nlp::nlp` is still `EncoderFixed`'s own live pitch estimator (not migrated).
`lpc.rs`/`voicing.rs` are single files holding both implementations side by side with shared test
infrastructure (`read_dump`, `fixture!`), so moving only the reference-only functions out doesn't
fully separate those files either way -- a real, honest trade-off, not free. Do this as its own
dedicated pass once the remaining migration work below is further along (or immediately, if picked up
before then) -- not silently forgotten, per Bruce's own explicit request to record it here.

## Explicitly not attempted this pass

Everything still marked NOT STARTED above: `lpc_energy`, `bw_gamma`, `build_p_q`, `fixed_point.rs`'s
`log2_lut`/`exp2_lut`, `quantise.rs`, and the fixed-point FFT (`nlp.rs`, still the largest single
remaining item). Real progress landed on the LPC-coefficient side of the chain (windowing through
Levinson-Durbin), but everything from LSP conversion's own input boundary onward is still `f32`, and
`nlp.rs`'s pitch estimator hasn't been touched at all. Do not report this punch list as closed without
re-reading this table against the actual current code -- per this project's own standing
"keep-working-until-actually-done" discipline, a status header can go stale; re-derive from `grep`ing
the real function signatures, not from this file's own prose, before trusting it.
