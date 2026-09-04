# Fixed-Point Encoder Implementation: Punch List

## Status: scoped 2026-09-04, per Bruce's direct request "Implement the fixed-point encoder." Real
## structure now exists (`floating_reference::Encoder` + a parallel `EncoderFixed`, both build and
## pass their own round-trip test), and `is_voiced_fixed` is genuinely wired into `EncoderFixed`. Two
## more candidates exist but aren't wired anywhere yet. The `autocorrelate` gate turned out harder
## than scoped: a real fixed-point normalization candidate was built and is mathematically sound, but
## fails this codebase's own acceptance discipline on frame 273 for the same structural reason
## `div_round_i128`'s reciprocal-multiply idea was already rejected -- see that row for the full,
## precisely-diagnosed finding. Everything else is real, unstarted, multi-session work. This file is
## the ground truth for what's actually done -- re-derive from here against the real code, not from a
## chat summary, before ever reporting this "complete."

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
| `autocorrelate` + `Autocorr` boundary | Full `f32`. `levinson_durbin_fixed_core`'s very first line already divides every `R[j]` by `r0` (in `f32`) -- cancelling the "shared exponent" a block-floating design would have tracked, so that framing was more machinery than the real problem needs. **A real, precisely-diagnosed negative result found this pass, not just a risk flagged**: built a candidate replacing that `f32` division with a fixed-point one (raw `R[]` quantized to a single wide Q20.43 `i64` format -- 20 integer bits for real max `R[0]` ~8.6e5, 43 fractional for real margin at the real min ~4e-6, both comfortably under `i64`'s 63 usable bits, confirming block-floating genuinely isn't needed), then `div_round_i128`-style fixed-point division for `r[j]/r0`. Ran it through the *existing* discriminator test (`levinson_durbin_fixed_diverges_from_float_only_at_measured_clamp_disagreement_frames`'s own methodology, applied to this candidate) against the real `codec2_r_dump.txt` corpus (362 frames): **fails, on exactly frame 273** (this corpus's own known worst-conditioned frame), by 0.083 -- with no clamp disagreement, at 40, 43, and (diagnostically) 70 fractional bits alike, ruling out a precision-budget explanation. **Root cause, confirmed precisely** (checked frame 273's own `r[1]/r[0]` against a `struct`-level `f32`-rounded division in Python): the *production* `f32` division has ~1.7e-8 relative error versus true double precision on this exact ratio, and frame 273 is fragile enough that even this ordinary, unremarkable rounding artifact is load-bearing for `levinson_durbin_fixed`'s own current passing status -- this candidate's wide-integer division is *more* mathematically exact, not less, and diverges anyway. Same structural fragility already flagged for `div_round_i128`'s own reciprocal-multiply idea (any change to a division's rounding right before the `1/e` amplification reshuffles which frames fall off the cliff), now confirmed to apply here too, empirically, not just by analogy. The failing test is kept in the tree (`r0_normalization_fixed_point_candidate_diverges_from_float_only_at_measured_clamp_disagreement_frames`, asserting the real, diagnosed `== 1` divergence count) as a real regression guard: if it ever starts passing at `== 0`, the candidate changed and needs rechecking, not just a loosened assertion. | Real bit budget confirmed sufficient (Q20.43 fits `i64` with margin) and block-floating confirmed unnecessary -- both real, useful, permanent findings. But no sound acceptance methodology exists yet for wiring in *any* fixed-point `r0`-normalization, the same open problem `div_round_i128`'s row already has. Would need either (a) the same small-`e` branch-to-wide-path design already recorded as a validated-but-unbuilt idea, applied one level earlier (detect `r0`'s own ill-conditioning before normalizing, not just `e`'s), or (b) a real, wider-than-clamp-disagreement acceptance bar for this specific stage, agreed with Bruce -- not resolved by this pass. Also still needs the real input-side bit budget for `autocorrelate`'s own accumulator (`wn[i]` = `i16` sample x Q0.30 window coefficient, summed over 320 terms) -- unmeasured, blocked on the normalization question above being resolved first (no point sizing the accumulator for a normalization step that doesn't have a sound acceptance test yet). | **NOT STARTED -- the real gate, and harder than originally scoped.** A real, working fixed-point `r0`-normalization candidate exists and is proven mathematically sound in isolation, but fails this codebase's own established acceptance discipline on the identical frame every other fragile stage in this file already struggles with. Nothing downstream in the LPC chain can wire in until this is resolved. |
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
