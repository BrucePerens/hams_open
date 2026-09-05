# Fixed-Point Encoder Implementation: Punch List

## Status: scoped 2026-09-04, per Bruce's direct request "Implement the fixed-point encoder." **As of
## 2026-09-04's follow-on pass, `EncoderFixed`'s entire encode path is genuinely fixed-point end to
## end -- no `f32` conversion of any real signal data anywhere in it.** The windowing ->
## `autocorrelate` -> Levinson-Durbin -> `lpc_energy` -> bandwidth-expansion -> LSP-conversion chain
## (including the LSP frequencies' own `acos()`, `lpc::acos_lut_fixed`) was already fixed-point as of
## the prior pass; this pass closed the one remaining piece, the pitch estimator (`nlp.rs`): `nlp::
## nlp_fixed` runs the same DC-notch/decimate/Hann-window/FFT/peak-search/sub-multiple-correction shape
## as the original `nlp()` entirely in `i64`/`i128` scaled-integer arithmetic, including a genuine
## fixed-point radix-2 FFT (`fft_fixed`, quantized Q23 twiddle table, no per-stage rescaling needed --
## `i64`/`i128` give far more dynamic-range headroom than a real block-floating-point FFT would need at
## this signal's actual magnitude), and returns `f0` (Hz) as `f32` only at its own single final
## boundary, so `quantise::encode_wo` needed no changes at all once wired to that boundary value.
## Validated against the already-tested float `nlp()` (agreement across a wide synthetic
## f0/harmonic-content sweep: 0% disagreement in the realistic range this crate's own original test
## already established) and, more importantly, against `floating_reference::Encoder` end to end:
## `EncoderFixed`'s decoded-audio correlation is **1.0**, same as before this pass, now with the *entire*
## encode path -- not just the LPC chain -- genuinely integer. This file is the ground truth for what's
## actually done -- re-derive from here against the real code, not from a chat summary, before ever
## reporting this "complete."

## Scope widened 2026-09-05, per Bruce's own explicit direction: "completely port codec2 3200 to
## fixed-point, so that it would actually work on a processor with no FPU." The encoder half above is
## done; the real remaining piece is a genuine fixed-point `DecoderFixed`, parallel to `Decoder`
## (mirroring the encoder's own build precedent: float kept live as a per-frame diff reference, fixed
## built and checked stage by stage against it) -- unlike the encoder, the decoder has **no design
## freedom**: it must reproduce the real reference's own dequantization/reconstruction exactly, since a
## real Codec2/Codec2-mod decoder or this crate's own decoder must land on the same real audio a real
## encoder's bitstream implies (`mod.rs`'s own doc comment already establishes this asymmetry). Once
## this is done, Bruce's own follow-on goal is Codec2 1600bps+data mode (used by M17) -- not started.

## DONE 2026-09-05: `DecoderFixed` built, wired, and validated end to end -- the full Codec2 3200
## fixed-point port (encoder + decoder) is now complete

Every decoder-side stage below is genuinely integer, no `f32` conversion of real signal data
anywhere, matching `EncoderFixed`'s own established bar. Built and validated front-to-back, largest
piece last, per advisor's own recommended order:

- **`quantise.rs` decode-side**: `decode_wo_fixed`/`decode_energy_fixed`/`decode_lsps_delta_scalar_fixed`,
  each genuinely integer in/out (Q23). `decode_energy_fixed` needed a real new primitive first --
  `fixed_point::log2_q23`/`exp2_q23`, genuinely integer-in/integer-out siblings of the existing
  `log2_lut`/`exp2_lut` (which are integer *inside* but still `f32` at their own boundary, since every
  caller there was still float upstream). `decode_lsps_delta_scalar_fixed` reuses the Q16 Hz-domain
  tables `encode_lsps_delta_scalar_fixed` already built.
- **`interp.rs`**: `interp_wo_fixed`/`interp_energy_fixed`/`interpolate_lsp_fixed` -- `interp_voiced`
  needed no fixed twin at all, being pure bool logic already. `interp_energy_fixed`'s geometric mean
  composes `log2_q23`/`exp2_q23` (`sqrt(a*b) == exp2((log2(a)+log2(b))/2)`) rather than a fixed-point
  square root.
- **`lpc.rs`'s `lsp_to_lpc_fixed`**: needed a genuinely integer-in/integer-out `cos_q23` (same 12-bit
  LUT shape `acos_lut_fixed` already established, but normalizing its `[0,pi]` domain into a Q23
  table-index fraction needs one real integer division, unlike `acos_lut_fixed`'s `[-1,1]` domain,
  which is already a Q23 fraction) plus `poly_mul_q23`/`build_half_poly_q23` (genuine Q8.23 siblings of
  `poly_mul_fixed`/`build_half_poly`, which were only "fixed" in the fixed-*size*-buffer sense, still
  `f32` throughout).
- **`envelope.rs`** (the largest stage before synthesis): `ModelFixed`, `compute_harmonic_amplitudes_fixed`,
  `apply_first_harmonic_correction_fixed`, `sample_filter_phase_fixed`. Needed a new, separate
  fixed-point FFT (`fixed_fft.rs`, below) and composes `log2_q23`/`exp2_q23` for every `sqrt`/`powf`/
  reciprocal (`r.powf(2*BETA)/a2` reduces in log domain to `a2g^BETA * a2^-(1+BETA)`, one shared helper
  for both the gain-normalization sum and the per-harmonic sum). The sub-1kHz postfilter boost's
  frequency threshold turned out to be an *exact* integer bin boundary (`1000Hz / 15.625Hz-per-bin ==
  64.0` exactly, given this codec's real `SAMPLE_RATE`/`FFT_ENC`), not something needing a LUT or
  rounding margin at all.
- **`fixed_fft.rs`** (new): a phase-correct fixed-point radix-2 FFT, deliberately *separate* from
  `nlp.rs`'s own `fft_fixed` even though both are the same butterfly shape at the same coincidental
  512-point size -- `nlp.rs`'s own version only ever reads magnitude/power, so its sign convention was
  deliberately left unpinned; this one needs real phase (`envelope::sample_filter_phase_fixed` reads
  `Aw[b].conj()` directly, `synthesis.rs` needs a real inverse transform), so its forward/inverse
  conventions are verified directly against `rustfft`'s own complex output, not just power -- including
  a forward-then-inverse round trip confirming the **unnormalized** inverse convention (no `1/N`
  divide) matches `rustfft`'s own. Also adds `ComplexQ23` (Q23 complex value, `conj`/`mul`).
- **`trig_fixed.rs`** (new): `sin_cos_q23`, genuinely integer `sin`/`cos` with the angle in a plain
  `u32` "turns" representation (binary angle measurement) rather than Q23 radians -- the full `u32`
  range is one turn, so a phase accumulator's own per-subframe advance (`wrapping_add`) and
  per-harmonic scaling (`wrapping_mul`, widened through `u64`) both fold angle wraparound into ordinary
  integer overflow, no modulo or float range-reduction anywhere.
- **`synthesis.rs`** (the largest and final stage): `synthesize_phase_fixed` (voiced-excitation phase
  tracking via `sin_cos_q23` and the `u32` phase accumulator; unit-vector normalization composes
  `log2_q23`/`exp2_q23` for `1/sqrt`), `postfilter_step_fixed`/`postfilter_fixed` (the `e_db`/`thresh`
  comparison ported directly to Q23 log-domain composition; `bg_est` carried as a genuine Q23 EMA
  across frames, not converted per call), `ear_protection_fixed` (`gain = (thresh/max_abs)^2` via one
  direct `i128` division, not a log-domain round trip -- the one path whose entire job is bounding
  amplitude, so exactness beats LUT economy), `SynthesisStateFixed`/`synthesize_subframe_fixed`
  (`fixed_fft::fft_fixed(forward: false)` in place of the `Arc<dyn Fft<f32>>` inverse plan; the Parzen
  window precomputed once to Q23 from the existing float construction, since table construction isn't
  the hot path).
- **`mod.rs`'s `DecoderFixed`**: mirrors `Decoder::decode`'s exact structure with every `_fixed`
  sibling wired in. No FFT-planner field at all (unlike `Decoder`'s own `Arc<dyn Fft<f32>>`) --
  `compute_harmonic_amplitudes_fixed` calls straight into `fixed_fft`, no trait object needed.

**Real acceptance bar, same one `Decoder`'s own test uses**: `DecoderFixed` decoded against the real
captured reference bitstream/PCM fixture (`synthetic_c_encoded_bits.bin`/`synthetic_c_decoded_pcm.bin`)
-- correlation **0.9964** (> 0.99) and RMS ratio **1.0005** (essentially exact scale match, confirmed
by direct measurement, not derived: an early design question was whether `envelope.rs`'s own amplitude
gain needed to absorb `rustfft`'s unnormalized-inverse `FFT_ENC` scaling factor somewhere, and the
measured ratio settles it -- no missing or extra factor anywhere in the fixed-point synthesis chain).

**Two real divergences found, checked, and accepted as design-choice-level, not bugs** (this module's
own doc comment already establishes decoder-internal quantities like harmonic count and postfilter
phase randomization are "not a bitstream format question... exact formulas are a design choice"):
- `ModelFixed::new`'s harmonic count (`l = pi_q23()/wo_q23`) can differ from `Model::new`'s
  (`l = (PI/wo) as usize`) by exactly one harmonic at `wo == W0_MIN` (where `PI/wo == 80` exactly in
  true arithmetic, but the two independent Q23/f32 roundings of `PI` and `wo` land on opposite sides of
  that boundary) -- checked exhaustively across all 128 real transmitted `Wo` indices, tolerance 1.
- The two decoders' unvoiced/postfilter-randomized harmonics pick different random phases for the same
  harmonic on the same frame (each decoder holds its own independent `rng: u32` seeded at `0xC0FFEE`;
  the float path derives an angle via `(state>>8) as f32/2^24*TAU`, the fixed path uses the raw
  post-step state directly as a `u32` turns angle) -- by design, not a bug: `next_rand`'s own doc
  comment already establishes this doesn't need to match the reference's generator, "purely a
  synthesis-quality detail, not transmitted." What *does* need to match (and was checked) is the
  *number and order* of draws per sub-frame, not the numeric values themselves.

Every new fixed-point primitive/module this pass added was validated against its own float sibling on
real captured data before being wired into `DecoderFixed`, then again as part of the acceptance test
above: `log2_q23`/`exp2_q23` (against `log2_lut`/`exp2_lut`, both directions, plus as real inverses of
each other), `cos_q23` (dense sweep of its whole `[0,pi]` domain against plain float `cos`),
`fixed_fft::fft_fixed` (both `forward` and inverse against `rustfft`'s own complex output directly, not
just power), `trig_fixed::sin_cos_q23` (dense sweep of the full `u32` range against plain float
`sin_cos`, plus an explicit wraparound-seam test), `compute_harmonic_amplitudes_fixed` (absolute
per-harmonic amplitude, not just shape, against real captured LSP/energy data -- the stage where real
amplitude scale first becomes load-bearing), `postfilter_step_fixed` (a real multi-frame temporal
replay comparing `bg_est` EMA drift, not just single-frame decisions, mirroring
`postfilter_lut_decisions_match_plain_float_across_a_real_temporal_replay`'s own methodology).

Two real bugs caught by this pass's own review discipline before they shipped, not found later:
one hand-typed Q23 literal (`1.96*2^23` in `envelope.rs`) was arithmetically wrong by 455 counts in a
first draft -- caught by re-deriving it, and fixed by switching every Q23 constant in this pass to
`f32_to_q_exact_round` computed once in a `OnceLock`, never a hand-typed literal (the same
`BW_GAMMA_Q23` lesson this project already learned once, relearned and this time generalized as a
standing practice for the whole pass). `fixed_point::exp2_q23` had no overflow guard on its
`floor(y)>=0` branch (a silent `i64` wrap past `floor(y)>=39`, the same failure class as the earlier
`correct_sub_multiples_fixed`/`CNLP*gmax` bug from the encoder pass) -- added a `debug_assert` before
any real caller could hit it.

**Next**: per Bruce's own explicit direction, Codec2 1600bps+data mode (used by M17) -- not started.

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
| `build_p_q` | Was: full `f32`. Now: `lpc::build_p_q_fixed(a_q23) -> ([i32;6], [i32;6])`, pure add/sub/double, no multiply, no widening. | Fixed-point port + validation against real captured `P[]`/`Q[]` data | **DONE and WIRED into `EncoderFixed`.** |
| `cheb_poly_eval_fixed` / `find_next_root` / `lpc_to_lsp` | Was: fixed-point internally, but input (`ak`) still arrived as `f32` from `build_p_q`. Now: `cheb_poly_eval_fixed_core`/`find_next_root_from_q23`/`lpc_to_lsp_from_integer_ak` take Q8.23 integer input directly -- the `f32`-facing originals became thin wrappers (quantizing once, not per-candidate-`x`, a real efficiency win alongside the refactor). LSP frequencies stay `f32`-*typed*, but the `acos()` call itself is now `lpc::acos_lut_fixed` (see its own row below), not a genuine float transcendental. | Real fixture-corpus validation, all the way through LSP root-finding | **DONE and WIRED into `EncoderFixed`.** 362/362 real frames found all 10 LSP roots (matching the float path's own robustness); max error 3.3e-4 rad even with the acos LUT wired in (looser than the pre-fixed-point float path's own 1e-4 bound -- real, different arithmetic, not a bug -- but only ~0.43Hz against the real 25Hz LSP quantizer step). |
| `fixed_point.rs`'s `log2_lut` | Was: real LUT-based approach, but the interpolation step itself was genuine `f32`. Now: `log2_lut_generic_fixed` -- the raw IEEE754 mantissa bits (`x.to_bits() & 0x007F_FFFF`) are already an *exact* Q23 fixed-point representation of `(mantissa - 1.0)`, so the index/weight split and the table blend (a quantized `log2_lut_table_q23`) run in pure `u64`/`i64` arithmetic; `f32` is touched only at the exponent-extraction input and the final `exponent + interp` sum. Signature stays `f32 -> f32` (every real caller -- `quantise::encode_energy`, `synthesis::postfilter_step` -- is still float upstream), but the interpolation itself no longer is. | Direct dense sweep against the float interpolation across 12 decades (not just quantizer-index agreement, which is coarse enough to hide a sign/off-by-one bug), plus explicit boundary tests (exact powers of two, just-below-a-power-of-two where the `.min(levels-1)` clamp is load-bearing) | **DONE and WIRED** -- `log2_lut()` itself now calls the fixed path; `quantise::encode_energy`/`decode_energy` and `synthesis::postfilter_step` get it for free with no caller change. |
| `fixed_point.rs`'s `exp2_lut` | Was: full `f32` (`floor()`, `2f32.powi()`, interpolation multiply/subtract). Now: `exp2_lut_generic_fixed` -- `y` (not IEEE754-shaped, unlike `log2_lut`'s `x`) is quantized once at the boundary (`y_q`, Q(8+16)), `floor(y)` and its remainder come from a plain arithmetic-shift (correct on negative `y_q`, confirmed by direct test against negative integers and just-below-a-negative-integer boundaries), the index/weight split mirrors `log2_lut_generic_fixed`'s own shape against a table storing `(2^frac - 1.0)` in Q23 (the raw-mantissa-bits format), and the final `mantissa * 2^floor(y)` is built as a direct IEEE754 bit pattern (`biased_exp<<23 \| mantissa_frac`), not `2f32.powi()`. | Direct dense sweep against the float interpolation across the real `y` range (`-20..20`, wider margin than `E_MIN_DB..E_MAX_DB` converted through log2 needs), plus explicit boundary tests (exact integers including negative ones, just-below-a-negative-integer) | **DONE and WIRED** -- `exp2_lut()` itself now calls the fixed path; `quantise::decode_energy` and `synthesis::postfilter_step` get it for free with no caller change. |
| `quantise.rs` (`encode_wo`) | Full `f32` | N/A -- its own arithmetic (`quantize_linear`) was already a trivial, already-integer-friendly linear formula; the real blocker was always its input | **DONE, unblocked by `nlp_fixed` below.** `encode_wo` itself needed no changes: its real input, `Wo`, now comes from `nlp::f0_to_wo(f0)` where `f0` is `nlp_fixed`'s own single `f32`-boundary output (the "integer core, float boundary" pattern, same as `lpc_to_lsp_from_integer_ak`'s `lsp`) -- wired into `EncoderFixed`, no `f32` conversion of `sn` anywhere upstream of it anymore. |
| `lpc.rs`'s `acos_lut_fixed` | Real gap identified from the `encode_lsps_delta_scalar` row's own corrected framing below: `lpc_to_lsp_from_integer_ak`'s trailing `.acos()` was the last genuine `f32` transcendental call in the LPC-analysis chain. Now: `acos_lut_fixed`, the same integer LUT shape as `log2_lut`/`exp2_lut` -- `x` (a Chebyshev root, not IEEE754-structured) is quantized via `fixed_point::f32_to_q_exact_round` (exact bit extraction, reused from the `exp2_lut` fix, not a float multiply); domain reduction (`acos(-x) = pi - acos(x)`) keeps the table over `[0,1]`; the final `pi - interp` combination runs as plain integer subtraction against a `pi_q23()` computed the same exact-bit-extraction way (avoiding the `BW_GAMMA_Q23`-style independent-literal mismatch risk). | LUT resolution (12 bits) picked from a *real measurement*, not assumed: candidate widths tested against real captured roots from `codec2_ak_dump.txt` (worst real root sits at `1 - \|root\| ~ 7.8e-4`) — 6 bits ~56Hz error, 8 bits ~28Hz, 10 bits ~5.3Hz, 12 bits ~0.09Hz, against the real 25Hz LSP quantizer step. A synthetic dense sweep across the *entire* `[-1,1]` domain (not just this corpus's own roots) then found the true worst case is larger than the corpus number -- ~7Hz, peaking near `1 - \|x\| ~ 5e-5..1e-4` (a uniform table genuinely isn't free near `acos`'s own singularity) -- still >3.5x under the real 25Hz bar, and an initial test written with too tight a rad-based threshold (picked from the corpus number alone) caught its own miscalibration when the dense sweep found this larger, still-acceptable, full-domain number; fixed by switching the acceptance bar to Hz (the real quantizer's own unit), not a flat rad threshold. | **DONE and WIRED into `EncoderFixed`** (via `lpc_to_lsp_from_integer_ak`, replacing its own `.acos()` call). |
| `quantise.rs` (`encode_lsps_delta_scalar`) | Was: full `f32`. Now: `encode_lsps_delta_scalar_fixed` -- `lsp[i]` (still `f32`-typed, per `lpc_to_lsp_from_integer_ak`'s own boundary) is quantized once via `fixed_point::f32_to_q_exact_round` (exact bit extraction), `HZ_PER_RAD` (`4000/pi`) folds into a one-time Q16 constant (`hz_per_rad_q16()`), and `LspDim`'s own `step1`/`step2`/breakpoint (always exact integer Hz in the real reference) become a plain integer Q16 table (`LspDimQ16`, no float conversion needed at all -- `25i64 << 16` is exact) -- `lsp_dim_nearest_level_q16`'s linear scan and comparison run entirely in `i64`. Signature unchanged (`f32 -> [u32; LPC_ORD]`), matching this pass's established "keep the caller-facing type, fix the interpolation/arithmetic inside" pattern. Earlier scoping in this file said this had "no real payoff" alongside `encode_wo`, which was wrong (advisor caught it) -- its real blocker was the `.acos()` call, now closed by `acos_lut_fixed`. | Real acceptance bar is index agreement, not tolerance: byte-identical `[u32; LPC_ORD]` against the float version on the real captured LSP corpus (`codec2_lsp_dump.txt`) -- any single-level disagreement would also shift every later dimension's own delta target. Plus a dedicated exact-tie test (`lsp_dim_nearest_level_q16_keeps_the_lower_index_on_an_exact_tie`), since exact ties are *more* likely in integer arithmetic than float (no rounding noise to break one by accident) and the float version's own doc comment records a real historical bug from getting this tie-break direction wrong once already. | **DONE and WIRED into `EncoderFixed`.** 0/362 real index mismatches (`encode_lsps_delta_scalar_fixed_matches_the_float_version_exactly_on_real_captured_lsp_data`); `EncoderFixed`'s own decoded-audio correlation against `floating_reference::Encoder` stayed **1.0** after wiring this in. Real follow-on, not done this pass: `lpc_to_lsp_from_integer_ak`'s own Q23 angle currently round-trips through `f32` (lossy for angles near `pi`, since values up to `pi*2^23` exceed `f32`'s exact-integer range of `2^24`) before this function re-quantizes it back to Q23 -- a `acos_lut_fixed_q23` core (mirroring the `cheb_poly_eval_fixed`/`find_next_root` wrapper/core split) handed through `lpc_to_lsp_from_integer_ak` directly would close that redundant round trip; not needed for correctness today since it's the same precision `EncoderFixed`'s own `lsp` field already carries, just a real, doable tightening. |
| `nlp.rs` (FFT-based pitch estimator) | Was: full `f32`, `rustfft`/`Complex32`. Now: `nlp::nlp_fixed(state, sn: &[i16; M_PITCH]) -> f32` -- same DC-notch/decimate/Hann-window/peak-search/sub-multiple-correction shape as `nlp()`, entirely `i64`/`i128` scaled-integer arithmetic (Q23 throughout: `notch_a_q23`, `lowpass_coeffs_q23`, `hann_window_q23`, `cnlp_q23`), plus a genuine fixed-point radix-2 DIT FFT (`fft_fixed`, quantized Q23 twiddle table via `fft_twiddles_q23`, bit-reversal via `fft_bit_reverse_table`). Key design realization (advisor-prompted before starting): `power[]` is used only for *ordinal* comparisons (peak search, `CNLP*gmax` threshold tests) -- absolute magnitude never escapes this module except the one final `f0` scalar -- so a uniform integer scale at every stage is free, and `i64`/`i128`'s own dynamic range (measured: real signals at this crate's own realistic amplitude never approach overflow even without any block-floating-point rescaling) removes the need for the per-stage rescaling a genuinely bit-width-constrained fixed-point FFT would require. | **Different, weaker bar than everything above** (per `nlp.rs`'s own module doc: a real decoder only ever sees the quantized `Wo` index, so "finds the same fundamental," not "reproduces the reference's arithmetic," was always the right bar) -- validated by direct front-end-first comparison against each float twin (`dc_notch_fixed`/`decimate_fixed`/`fft_fixed` each checked against `dc_notch`/`decimate`/a real `rustfft` FFT independently, before composing them), by `nlp_fixed` vs `nlp` agreement across a wide synthetic f0/harmonic-content sweep (0% disagreement in the realistic range `nlp()`'s own original test already established), and by two dedicated exact-tie tests pinning `correct_sub_multiples_fixed`'s own strict-`>` local-peak comparison (a real, reachable case where float and fixed-point can legitimately land on different sides of a tie -- documented as expected behavior, not a bug, matching this crate's own precedent for the LSP quantizer's exact-tie case). | **DONE and WIRED into `EncoderFixed`.** `EncoderFixed`'s own decoded-audio correlation against `floating_reference::Encoder` stayed **1.0** after wiring this in -- the entire encode path, not just the LPC chain, is now genuinely fixed-point end to end. |
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

## DONE 2026-09-04: moved all floating encoder code into `floating_reference/`

Per Bruce's own explicit follow-up request ("Move all of the floating encoder code to
floating_reference"). `floating_reference/` is now a directory mirroring `codec2_3200`'s own
top-level layout, one submodule per parent module the float implementations moved out of:
`floating_reference::lpc` (`autocorrelate`, `apply_white_noise_correction`, `levinson_durbin`,
`build_p_q`, `find_next_root`, `lpc_to_lsp`, `lpc_energy`), `floating_reference::nlp` (`NlpState`,
`nlp`, `decimate`, `correct_sub_multiples`, `dc_notch`), `floating_reference::quantise`
(`encode_lsps_delta_scalar`, `lsp_dim_nearest_level`), `floating_reference::voicing` (`VoicingState`,
`is_voiced`). Each parent module (`lpc.rs`, `nlp.rs`, `quantise.rs`, `voicing.rs`) keeps only what its
own fixed-point production code, or the one shared `Decoder`, still needs directly -- exported back
via `pub(crate)`, documented in each parent module's own updated doc comment:
- `lpc.rs` keeps `find_next_root_from_q23`/`COEF_FRAC_BITS` (its own `lpc_to_lsp_from_integer_ak`
  calls the former directly) and `lsp_to_lpc`/`Autocorr`/`LpcCoeffs` (the shared `Decoder`'s own LSP
  reconstruction, and the crate-wide type aliases both sides use -- neither was ever part of this
  move, confirmed by checking real call sites, not assumed from the punch list's own earlier text).
- `nlp.rs` keeps `lowpass_coeffs`/`LPF_TAPS`/`NOTCH_A`/`CNLP`/`NDEC` (its own `*_q23` table builders
  read these directly) and `f0_to_wo` (genuinely shared, unchanged).
- `quantise.rs` keeps `LspDim`/`LSP_DIMS`/`LSP_LEVELS`/`lsp_dim_value_hz` (`decode_lsps_delta_scalar`,
  the shared `Decoder`'s own function, also uses these).
- `voicing.rs` needed no exception at all -- `VoicingStateFixed`/`is_voiced_fixed` were already a
  fully independent struct/function pair, the cleanest of the four moves.

**Verified with a hard invariant throughout, not just "it compiles"**: the crate's own lib test count
(147) never changed across the whole move. Every test in each moved file was individually classified
into exactly one of three buckets before moving anything: (a) tests solely of the moving float
function -> moved with it; (b) genuine float-vs-fixed cross-validation tests (e.g. `lpc_energy_fixed_
and_lpc_energy_produce_the_same_real_quantizer_index`, `lsp_to_lpc_round_trips_lpc_to_lsp_on_real_
captured_ak_data`) -> stayed in the fixed-point file, importing the moved float function back; (c)
tests of fixed-point-only code or genuinely unrelated -> untouched. Shared fixture-parsing
infrastructure (`read_dump`/`fixture!` in `lpc.rs` and `quantise.rs`) was *not* duplicated -- each
parent module's own `mod tests` was made `pub(crate)` and its `fixture!` macro re-exported via
`pub(crate) use fixture;` (a `macro_rules!` can't take a visibility qualifier directly), so
`floating_reference`'s own tests import and reuse the same real fixture-reading code rather than
maintaining a second copy that could drift. `nlp.rs`'s `NlpState` (previously a single struct mixing
both float and fixed fields, from the earlier `nlp_fixed` pass) was split cleanly into two independent
structs -- `floating_reference::nlp::NlpState` (float-only fields) and `nlp::NlpStateFixed`
(fixed-only fields, needing no FFT-planner field at all, since `fft_fixed` is a from-scratch radix-2
implementation, not `rustfft`) -- a real, deliberate breaking API change, permitted per this project's
own standing "not yet deployed, breaking changes OK" rule.

One real, honest gap found and left as a separate, later exception: `window::make_analysis_window`
(the `f32` version) is still called *inside* `make_analysis_window_fixed` itself to derive its own
quantized table, so it couldn't move -- the one case among all four modules where the float
implementation is a genuine, ongoing dependency of fixed-point production code, not just its own
historical validation target.

## Remaining, as of the `nlp.rs` pass (2026-09-04 follow-on)

Every row in the table above is now **DONE and WIRED**. `EncoderFixed`'s entire encode path -- windowing,
autocorrelate, Levinson-Durbin, `lpc_energy`, bandwidth expansion, LSP conversion, the LSP delta-scalar
quantizer, the pitch estimator (including its own fixed-point FFT), and `encode_wo` -- runs with no
`f32` conversion of real signal data anywhere in it; the only `f32` left in `EncoderFixed` are the
established "integer core, float boundary" single-scalar handoffs (`lsp: [f32; LPC_ORD]`, `f0: f32`)
that downstream code already consumes as `f32` and has no reason to change. Two real, honest, smaller
follow-ons left open, neither a correctness gap:
- Recorded in the `encode_lsps_delta_scalar` row above: `lpc_to_lsp_from_integer_ak`'s Q23 angle
  round-trips through a lossy `f32` conversion before `encode_lsps_delta_scalar_fixed` re-quantizes it
  -- closing that redundant round trip needs an `acos_lut_fixed_q23` core handed through directly, not
  yet built.
- Pre-existing, not a regression: `lpc.rs`'s own `f32_to_q` helper, and `nlp.rs`'s own `f32_to_q23`
  (both used only for one-time coefficient/table quantization at program start, never per-sample) do
  `x as f64 * ... .round() as iNN` -- the same "claims fixed-point, does an f64 multiply" shape the
  `exp2_lut` fix caught and corrected for a genuine *per-sample* hot path in that file; here it's a
  one-time table-construction cost, the same convention `acos_lut_table_q23`/`lowpass_coeffs_q23`/
  `hann_window_q23`/`fft_twiddles_q23` all already use, left as a known, low-priority stylistic note
  rather than fixed mid-stream.
- Also noted, not part of `EncoderFixed`'s own encode path: `nlp_fixed`'s validation sweep intentionally
  excludes a pure, harmonic-free tone right at the pitch range's own edge (~380Hz, near `P_MIN`'s ~400Hz
  limit) -- checked directly, even `nlp()` alone is only marginally accurate there (~3.6% error against
  the true frequency) and real voiced speech always carries harmonics, so an agreement bound between
  two already-only-approximately-accurate estimates at that specific edge case wasn't a fair or useful
  bar (see `nlp_fixed_agrees_with_the_float_reference_across_a_wide_synthetic_sweep`'s own doc comment).

Do not report this punch list as closed without re-reading this table against the actual current
code -- per this project's own standing "keep-working-until-actually-done" discipline, a status
header can go stale; re-derive from `grep`ing the real function signatures, not from this file's own
prose, before trusting it.
