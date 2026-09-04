# Fixed-Point Encoder Implementation: Punch List

## Status: scoped 2026-09-04, per Bruce's direct request "Implement the fixed-point encoder." Two
## stages landed this pass (window, voicing threshold); the rest is real, unstarted, multi-session
## work. This file is the ground truth for what's actually done -- re-derive from here, not from a
## chat summary, before ever reporting this "complete."

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

## Open product decision -- Bruce's own call, not resolved by this pass

**Replace `Encoder` in place, or build a parallel `EncoderFixed`?** `hams-not-yet-deployed-breaking-
changes-ok` permits an in-place replacement (nothing is deployed yet). But the validation discipline
that made every real win this session possible -- `cheb_poly_eval_fixed`'s bit-exactness proof
against the live `f32` implementation, `levinson_durbin_fixed`'s clamp-disagreement discriminator
against `levinson_durbin` -- depends on having the `f32` path live to diff against, frame by frame,
for as long as validation is incomplete. A parallel `EncoderFixed` keeps that reference available
through the whole build; replacing `Encoder` outright removes it the moment the swap happens, before
every stage has its own passing cross-check. Recommend parallel, at least until every stage below
is DONE -- but this is a real product-shaped decision, not a technical one, and it's Bruce's call.

## Punch list -- stage, current state, real acceptance bar, done or not

| Stage | Current state | Real acceptance bar | Status |
|---|---|---|---|
| `window.rs` (`make_analysis_window`) | Computed at runtime in `f32`, but the window shape is a compile-time constant | Quantize the constructed window to a fixed-point output type with real measured margin | **DONE, this pass.** `make_analysis_window_fixed() -> [i32; M_PITCH]` added, Q0.30 (real measured peak ~0.0043, real margin under `i32`), validated against the real `f32` construction (max dequantized error < 1e-8). The one-time `cos()` construction itself stays `f32` internally (runs once per `Encoder` lifetime, not per-frame -- real but negligible cost); what changed is the *output* type, which is what the eventual per-frame consumer needs. **Not yet wired into `Encoder`.** |
| `voicing.rs` (`is_voiced`) | Corrected after starting: `energy_db = 10*log10(energy)` is a real dependency on the still-incomplete `log2_lut` (not just a threshold to narrow) | See resolution below | **DONE, this pass, via a real design change** -- see below |
| `autocorrelate` + `Autocorr` boundary | Full `f32`, output feeds `levinson_durbin_fixed` (which currently normalizes by `r[0]` in `f32` before its integer core) | Block-floating fixed-point representation with a shared exponent (`R[0..10]`'s own real measured range is ~10^11 across real speech, `CODEC2_MOD_FIXED_POINT_PLAN.md`'s own per-stage table) -- validate bit-exact or documented-divergence against `codec2_r_dump.txt`, record the real measured exponent range | **NOT STARTED -- the real gate.** Nothing downstream in the LPC chain can wire in without an `f32` round-trip until this boundary exists. |
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
settled noise floor -> not voiced) -- all four matched. **Not yet wired into `Encoder`** (its input
would need to be raw `i16` samples, not the `f32`-converted `self.sn` `Encoder::encode()` currently
builds).

## Explicitly not attempted this pass

Everything marked NOT STARTED above. In particular: the `autocorrelate`/block-floating boundary
(the real gate on the rest of the LPC chain) and the fixed-point FFT (the largest single remaining
item) are both real, substantial, multi-session pieces of work, not started here. Do not report this
punch list as closed without re-reading this table against the actual current code -- per this
project's own standing "keep-working-until-actually-done" discipline, a status header can go stale;
re-derive from `grep`ing the real function signatures, not from this file's own prose, before
trusting it.
