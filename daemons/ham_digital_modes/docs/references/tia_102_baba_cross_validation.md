# TIA-102.BABA (Project 25 full-rate IMBE) cross-validation findings

Scope: the floating-point implementation in `src/ambe/float/tia_102_baba/` (Project 25 Phase 1, 7200 bits per
second on the air, 4400 of them speech, 144 bits per 20 ms frame), checked against independent decoders. The
Digital Voice Systems (DVSI) chip cannot serve as an oracle: its `RATET(27)` / "P25 FEC" setting is a different
proprietary codec (see `AMBE_CHIP_VALIDATION_FINDINGS.md`). Date of the first pass (decoders): 2026-09-19. No network
access and no chip were used. A second pass the same day, against the OP25 `imbe_vocoder` encoder and decoder, is
recorded in the last part of this document ("Second pass: encoder oracle").

## Oracles and tie-breaker (exact identities)

| Role | What | Where |
| --- | --- | --- |
| Oracle A (decoder, ISC licence) | mbelib 1.3.0 (`MBELIB_VERSION` in `mbelib.h`), git commit `9a04ed5c78176a9965f3d43f7aa1b1f5330e771f` (from `packed-refs`) | `/tmp/bughunt/mb/` (identical sources; a second checkout is at `/home/bruce/workspace/tmp/mbelib_research/mbelib`). Built in a scratch directory outside the repository with a small C driver that feeds the eight code vectors through `mbe_processImbe7200x4400Framef` and prints the parameters. |
| Oracle B (decoder, MIT licence) | kchmck `imbe.rs`, commit `2e17f5a9eb2cd69fa1e243f5b16b1321172ac7ba`, plus the two local changes listed in its `VENDORED.md` (dependency upgrades only, no arithmetic) | `/home/bruce/workspace/hams_com/reference/ambe/imbe.rs/`. Used as an oracle and for constants; no code copied. |
| Tie-breaker | TIA-102.BABA "Project 25 IMBE Vocoder Description" (2003), 138 PDF pages; printed page number = PDF page minus 16 | `/home/bruce/workspace/hams_com/reference/ambe/TIA-102.BABA_Project_25_IMBE_Vocoder_Description.pdf`. Pages read as images: 41-42 (Eq. 50-51), 46-48 (Eq. 61-64, Tables 3-4), 52-53, 57-59 (Eq. 81-85, generator matrices), 76-77 (Eq. 127-138), 86-89 (Annexes E and F). |

Both oracles omit or change parts of the standard, so only parameters that the standard fully defines were
compared to numerical precision: fundamental frequency, harmonic count, band count, per-harmonic voicing, and
the dequantized spectral amplitudes (before enhancement). Synthesized audio was compared only coarsely.

Test material: our own encoder (`tia_102_baba::encoder::Encoder`, standard linear pitch map) on the first 300
frames of each of `OSR_us_000_00{10,11,30,31}_8k.wav`; plus a synthetic sweep of 8649 frames built from raw
quantizer indices (every `b0` in 0..=207 with extreme and random field values, exhaustive per-field sweeps of
`b2`, each gain element and each higher-order coefficient at L = 9, 20, 37 and 56, and a 3000-frame random
stream with no resets to exercise the prediction across every change of harmonic count). The harness lives in
`examples/tia_102_baba_cross_validation_dump.rs`; the C and Python glue was scratch and is not kept.

## Result in one line

After the two changes below, this crate agrees with mbelib (with its one wrong table entry corrected) on all
1200 speech frames and all 8649 sweep frames: identical `b0` derived values, L, K and voicing; `log2` amplitudes
within 2.3e-5 and linear amplitudes within 1.7e-4 relative (mbelib works in single precision); enhanced
amplitudes within 2.2e-4. It agrees with `imbe.rs` everywhere except the voicing expansion for L above 36 (an
`imbe.rs` bug, below).

## Divergences and dispositions

| # | Divergence | Disposition | Evidence |
| --- | --- | --- | --- |
| 1 | **Our wire layer was not the standard's.** `encode_code_vectors` used the DVSI chip's Hamming labelling and skipped the pseudo-random modulation of Eq. 84-94; `DecoderState` skipped the matching demodulation. A real over-the-air P25 frame would have decoded to garbage. | **Ours wrong. Fixed.** Standard framing is now the default (`encode_code_vectors`, `decode_code_vectors`); the chip framing moved to `*_chip` variants (`DecoderState::new_chip_wire`, `Encoder::new_chip_wire`, `encode_frame_chip_wire`) used only by the chip-comparison examples and tests, in both the float and fixed trees. | Generator matrix `g_H` (page 42-43 of the printed standard) equals the parity rows in `general::fec::HAMMING_PARITY`; Eq. 84-85 modulation. mbelib and `imbe.rs` both demodulate. Our frames decode in mbelib with zero errors. |
| 2 | **mbelib Annex F row L = 23, `b_hat_6` step is 0.068.** | **mbelib wrong** (typo); ours correct. Recorded only; not reported upstream (project rule: no outside reports without Bruce's review). | Standard Annex F, page 72: 4 bits, 0.058000 (same as L = 22 and 24). `imbe.rs` `gain.rs` also has 0.058. All 240 Annex F rows of ours match the standard's text layer mechanically. With the one-line patch applied to a scratch copy, every amplitude difference vanishes. |
| 3 | **`imbe.rs` voicing map for L above 36.** `gen_harmonics_bitmap` builds 3K bits and shifts down for L not a multiple of 3, so with K = 12 fixed the first L-36 harmonics read past the map. | **`imbe.rs` wrong**; ours and mbelib correct. | Eq. 50: `kappa_l = 12` for `l > 36`, so harmonics 37..L all take band 12's bit (page 26). 2026 sweep frames disagree with `imbe.rs`, all with L > 36, none below. |
| 4 | **Eq. 62/63 saturation boundaries** (the "UNRESOLVED" item in the plan). | **Ours correct. Resolved.** | Pages 46-47: index is 0 if `floor(x/step) < -2^(B-1)`, `2^B - 1` if `>= 2^(B-1)`, else `floor(x/step) + 2^(B-1)`. `quantize.rs::saturating_uniform_quantize` is that, branch for branch. Dequantization `step * (b - 2^(B-1) + 0.5)` matches both oracles. Tables 3-4 (step multipliers, standard deviations), Annex E (all 64 levels), Annex G widths and Annex J block lengths match both oracles exactly. |
| 5 | **Output level.** Ours is about the input level (RMS 1763 against an input of 1800); mbelib's is half of that, about 6 dB lower in every band. | **mbelib non-conforming**; ours follows the standard. | Eq. 127 (page 60) has an explicit factor 2 on the voiced signal, which mbelib omits. (Its later "gain of 7" is a playback scaling, unrelated.) |
| 6 | **mbelib's Hamming(15,11) corrects single errors wrongly for bit positions 2 to 7** (12288 of 30720 single-error words), although it accepts all 2048 valid words. | **mbelib wrong** (its syndrome-to-position table). | Tested against our textbook encoder. Golay correction agrees. With Hamming errors withheld, 3-error-per-Golay-block noisy frames decode identically to ours. |
| 7 | Synthesis differences: mbelib omits Eq. 111-116 smoothing and uses a different multi-sine unvoiced synthesis. | Expected; not a defect of ours. | Envelope correlation (log frame RMS) 0.990-0.998 over all four files; band-shape and waveform comparison is not meaningful against these differences. |

## Harness artifacts (so nobody chases them again)

* The first `imbe.rs` comparison showed phantom differences of up to 6.6 in `log2` amplitude: the "reset decoder
  state" marker of the sweep was not passed to the `imbe.rs` driver. Fixed in the scratch driver; not a codec issue.
* Frame 0 of each speech file differed at first only because of divergence 2 propagating through the prediction.
  The decay of that error (0.07, 0.04, 0.023, ...) is the signature of Eq. 77's feedback with rho near 0.5.

## Tests added

* `tests/ambe_tia_102_baba_cross_validation.rs`: Annex F L = 23 guard, Eq. 50/51 voicing guard for L above 36,
  and six golden frames (three chains: L changing 54, 9, 23, 21; L = 56 with saturated indices; L = 40) whose
  expected parameters were measured from mbelib. Golden numbers are this project's own measurements.
* Unit tests in `mod.rs` for the standard wire layer (modulated textbook FEC, correction up to capacity in every
  block including `c_hat_0`) and for the retained chip layer.
* `tests/ambe_tia_102_baba_chip_hamming_labeling.rs` and the fixed mute test were updated to use the chip
  framing or standard-conformant worst-case frames respectively; no assertion was weakened.

## Second pass: encoder oracle (2026-09-19)

The first pass validated the decoder half and the wire layer only. This pass validates the encoder (analysis,
quantization, bit prioritization) and, secondarily, the decoder again against a third independent implementation.

### Oracle identities

| Role | What | Notes |
| --- | --- | --- |
| Oracle C (encoder and decoder) | OP25 `imbe_vocoder`, Pavel Yazev's fixed-point IMBE, in `boatbod/op25`, commit `71abcd0ead32f86f51615ea6cc8a6a4dba4c949a`, directory `op25/gr-op25_repeater/lib/imbe_vocoder/` ("Version 1.0 (c) 2009", GPL-3 or later) | A shallow clone, built in a scratch directory outside the repository with `g++ -O1` and a small driver (`enc`: PCM in, per-frame stages out; `dec`: bit vectors in, PCM and amplitudes out; `pri`/`unpri`: its frame packer and unpacker on given quantizer values; `sa`: its amplitude quantizer on given amplitudes). The driver reads the class's private analysis buffers through a `#define private public` so that `E(P)` can be dumped; two source files were copied into the scratch directory with debug prints added (`pitch_ref.cc`, `v_uv_det.cc`). Nothing was copied into or translated for this repository. The library works on the eight bit vectors `u_hat_0..u_hat_7` (before FEC); it has no FEC, no error estimation and no repeat or mute policy beyond "`b_hat_0` above 207 repeats the previous frame". |
| Oracle D (decoder only) | JMBE, `DSheirer/jmbe`, commit `8ba19fba7449482cb88efcf12eb345ff3ea17dc0`, GPL-3 | `codec/src/main/java/jmbe/codec/imbe/` does contain a full IMBE decoder (and `ambe/`, `ambeplus/`), and no encoder. **Built and run** on 2026-09-20 (plain `javac`, a Temurin 21 JDK and slf4j, JTransforms, JLargeArrays and commons-math3 from Maven Central; `tools/jmbe_cross_check/`; its `ambeplus/` package is not wired in, its AMBE+2 path is the 3600x2450 layout, which is ours). On 800 frames of one speaker it agrees with our decoders on the frame-energy envelope (IMBE 0.997, AMBE+2 0.992) with a constant level offset (JMBE's IMBE output is exactly 2x ours, the missing factor of 2 in voiced synthesis that mbelib shares; JMBE's AMBE+2 output is about 15 dB above ours) and one speech-onset frame (184) where JMBE is 4 to 7 dB louder than ours after removing the offset (the chip-validated AMBE+2 and D-STAR paths agree with the chip, not with JMBE). It found one real defect on our side, fixed the same day: our encoders left the C0 parity bit at 0 while the chip sets even parity over the extended Golay field (200 of 200 captured chip frames, both modes). Under errors JMBE and our default policy conceal to within 0.05 to 0.5 dB envelope distance of each other. Its source was read for the damaged-frame policy only (below). |
| Tie-breaker | TIA-102.BABA, printed page = PDF page - 16; pages read as images this pass: 37 (Eq. 43-44), 47 (Eq. 62-64, Tables 3-4), 62 (Eq. 97-98) | plus the text layer of pages 24-35 and 61-63 for the prose |

Test material: the first 300 frames of each of `OSR_us_000_00{10,11,30,31}_8k.wav` (1168 comparable frames), plus about 25
synthetic signals: silence, sinusoids at 150, 440, 1000, 2000 and 3500 Hz, harmonic signals with periods 22 to 120
(integer and non-integer), pulse trains, DTMF pairs, white noise at two levels, a pitch glide. Scripts and the
driver were scratch and are not kept; the examples that generate this crate's side are:
`examples/tia_102_baba_oracle_encode_dump.rs` (per-frame pitch, `L`, voicing, `b0..b_{L+1}`, `u_hat`, optionally
`E(P)`, `E_R`, `D_k`), `..._oracle_decode_dump.rs`, `..._oracle_bits_dump.rs`, `..._oracle_quant_dump.rs` and
`..._float_fixed_parity_probe.rs`.

**Alignment.** The oracle analyses frame `k` two frames late (it needs two frames of look-ahead), centred 311 samples
before the first sample of the 160-sample block just pushed, and its initial pitch estimate runs on a causally low-pass filtered copy (a
21-tap symmetric FIR, 10 samples of delay, all in `pe_lpf.cc`) so that estimation window is centred 10 samples earlier
than its refinement, voicing and amplitude window. The standard (Fig. 8) requires the centres to coincide, as ours do.
For comparisons oracle frame `k` is set against ours `k - 2` with our analysis centre offset by `-1` (the pitch window
centre; everything downstream is insensitive to offsets of +-10). At that offset `E(P)` agrees to 0.0004 mean absolute
difference on voiced-ish candidates (0.0019 on the other file), and the offset scan has its sharp minimum there.

### Result in one paragraph

After the fixes below, on 1168 speech frames: the initial pitch estimate is identical (within 2 half-sample steps) on
99.4% of the frames where the oracle's `E(P_hat_I)` is below 0.3 (voiced frames), 93.5% overall (the rest are unvoiced
frames whose pitch is arbitrary), and on synthetic signals with a clear pitch `P` and `L` are identical, `b0` is equal or one step off (row 11) and the gain index `b2` is
identical. The bit prioritization and its inverse are bit-exact on 2880 frames covering every `L`. The amplitude
quantizer is identical to the oracle's up to the one systematic difference in row 14. The decoder's enhanced
amplitudes equal the oracle decoder's to 0.4% wherever the oracle's 16-bit integers have resolution.

### Divergences and dispositions (continuing the first pass's numbering)

| # | Divergence | Disposition | Evidence |
| --- | --- | --- | --- |
| 8 | **The encoder never applied the input high-pass filter of Eq. 3** (`H(z) = (1 - z^-1) / (1 - 0.99 z^-1)`), which removes residual DC before any analysis. | **Ours wrong. Fixed** in both trees (`Encoder::push_samples`; integer Q16 state with a Q30 pole in the fixed tree; the bare `FrameAnalyzer`, shared with the D-STAR and AMBE+2 encoders, is unchanged). | With DC in the input, quiet frames look periodic at every lag. With everything else held equal, initial-pitch agreement with the oracle, in the shipped configuration (floored `E(P)`, no upper cap), went 0.821 -> 0.935 over all frames (voiced frames 0.937 -> 0.994), and the mean `E(P)` difference 0.067 -> 0.002 on one file and 0.029 -> 0.0004 on another. The oracle applies the same filter (`dc_rmv.cc`). Regression: `a_dc_offset_does_not_make_noise_look_periodic`. |
| 9 | **`E(P)` (Eq. 5) can leave `[0, 1]`**: a few thousandths negative on a perfectly periodic signal (harmonic period 40: -31/4096), above 1 on noise. The standard does not say what to do; the oracle clamps to `[0, 1]`. Negative values make the ratio tests of Eq. 18-19 ill-defined. | **Ours changed: floored at zero**, ratio tests multiplied out (`CE_F(cand) <= 1.7 CE_F(P0)`, no quotient, so a zero reference is fine). The upper cap is **not adopted**: it turns every noisy frame into exact ties which the float and fixed trees then resolve differently (it lifted overall initial-pitch agreement from 0.935 to 0.990 but changed nothing in voiced frames, 0.994 to 1.000, and cost the float/fixed parity tests). Spec ambiguity. | The float and fixed trees both floor (`E` in Q16.16 clamps at 0). Regression: `the_error_function_is_never_negative`. |
| 10 | **Oracle: initial-pitch window 10 samples earlier than the refinement window** (causal pitch low-pass filter, see Alignment). | **Oracle deviates from Fig. 8**; not replicated. | Once our (coinciding) windows are aligned to the oracle's pitch window, initial-pitch agreement is 0.991 (1.000 on voiced frames, measured with the `E(P)` upper cap of row 9 applied) whether or not our refinement window sits 10 samples from the oracle's: the stagger has no measurable downstream effect. (An agreement of 0.862 seen earlier was a 10-sample misalignment of the pitch window, not an effect of the stagger.) Recorded only; not reported upstream. |
| 11 | **Oracle refines pitch to an eighth of a sample** (19 candidates `P_I - 9/8 .. P_I + 9/8` in steps of 1/8, including `P_I` itself); Section 5.1.5 specifies ten candidates, the odd eighths, for quarter-sample resolution. | **Oracle finer than the standard**; ours follows the standard. Consequence: on an exact half-sample period (say 90.0) ours cannot land on it (it picks 89.875 or 90.125) so `b0` (half-sample steps) can differ by one from the oracle, and among voiced frames with the same initial pitch `b0` is equal in 61% and within one step in 87%. | Over 40 random harmonic signals of known period the oracle's refined period has rms error 0.048 sample, ours 0.080 (quarter-sample quantization is 0.072); with ours given the oracle's 19 candidates 0.0435. Distribution of oracle refinement offsets (all 19 values occur, ours only the 10 odd ones). |
| 12 | **V/UV: oracle's voicing measure `D_k` is systematically larger** in weak high bands, so it declares them unvoiced more often (395 band decisions voiced by ours and unvoiced by the oracle against 21 the other way, on 288 voiced frames with equal `b0`; per-band disagreement grows from 1% in band 1 to 62% in band 12). Thresholds `theta(k, omega0)` and `M(xi)` agree (correlation 0.98 and 1.000). | **Oracle numerics; ours conforms.** Recorded only; not reported upstream. | An independent numpy transcription of Eq. 24-36 written straight from the PDF text gives `D_k` equal to ours to three digits on both frames tried (0.003 0.327 0.730 0.466 0.77 against oracle 0.003 0.563 1.000 0.820), so this is not a shared misreading. On clean synthetic harmonic signals with a steep spectral tilt the oracle's `D_k` reaches 0.43 where ours stays at 0.20 (flat spectra: both 0.03 to 0.05); consistent with 16-bit fixed-point dynamic range. Truncating `W_R` the way its window table is truncated does not explain it. Threshold `E(P_hat_I) > .5` (Eq. 37) is `.55` in the oracle (`CNST_0_55_Q4_12`). |
| 13 | **Spectral amplitude estimates** (Eq. 43-44): checked the estimator constants (voiced and unvoiced) through the final decoded amplitudes. | **Agree.** | Decoded enhanced amplitudes with an oracle integer value of 50 or more (its unit is `4 M`): median ratio 3.99 over all four files (`log2` 1.996), 10th to 90th percentile within 0.3%; white noise (all unvoiced) mean bias -0.05 `log2`; synthetic harmonic signals with known amplitude: ours 0.22 rms `log2` from truth, the oracle 0.45. Below that (`M` under 12) the oracle's 16-bit integers truncate. |
| 14 | **Uniform quantizer: the oracle rounds, the standard floors.** Eq. 62-63: `b = floor(x / step) + 2^(B-1)`; oracle `qnt_by_step` rounds `x / step` to the nearest integer and dequantizes with the same `+0.5` step as Eq. 67-68, so its quantizer is biased by half a step. | **Oracle wrong (or at least non-conforming); ours conforms**; recorded only, not reported upstream (page 47 of the PDF read as an image: floor in all three branches). | Same amplitudes into both quantizers, 2000 random frames (all `L`, chained history and initial-state runs): oracle index minus ours is 0 in 72.4%, +1 in 27.6%, -1 in 0.02% (fixed-point rounding), mean +0.28; everything else (prediction with `rho`, blocks, DCTs, step sizes, Annex E-J) agrees. With rounding substituted temporarily in ours: identical in 99.4% (initial state) and 98.1% (chained), mean bias -0.003. Regression: `amplitude_quantization_equals_the_oracles_except_that_it_floors_where_the_oracle_rounds` (7 frames, numbers only). |
| 15 | **Bit prioritization and the frame packing** (Fig. 22). | **Agree exactly.** | 2880 random frames, 60 per `L` from 9 to 56, with extreme and random values, both directions (our `prioritize_bits` against its packer, its unpacker against our vectors): identical. The oracle also writes a stale `b_vec[L+2]` as the synchronization bit (the encoder never sets it), harmless here. Regression: `bit_prioritization_matches_the_oracle_packer_and_unpacker` (6 frames). |
| 16 | **Silence: `log2(0)`.** Eq. 54 takes the logarithm of amplitudes that are zero for digital silence. Ours produced `-inf` and NaN through the block DCT (leaving `b2 = 0` and arbitrary mid-range indices); the oracle treats a non-positive amplitude as `log2 = 0` (`M = 1`, its gain index is 17). | **Spec silent; ours changed to floor amplitudes at 1.0** (one PCM step) in both trees. Silence now encodes with the oracle's gain index and decodes to noise of RMS 11.9, as the oracle's does (ours had decoded to 1.7 through the NaN path). | Regression: `silence_gain_index_matches_the_oracle`, `silence_encodes_to_the_lowest_pitch_index_with_nothing_voiced` (both encoders walk the pitch down by the 20% limit of Eq. 10 each frame, 118 86 61 41 25 12 2 0, identical). |
| 17 | **Degenerate periodic inputs.** Pure sinusoids, DTMF pairs, low-level noise. | **Spec ambiguity, both self-consistent; not changed.** | See the table below. A pure tone is periodic at every multiple of its period, so `E(P)` is near zero at many candidates (both encoders' `E` arrays agree to 8/4096 on the 440 Hz tone) and the sub-multiple rules (Eq. 18-20) decide on differences below the fixed-point resolution. |
| 18 | **Decoder: amplitude and level.** | **Agree.** | Enhanced amplitudes of four oracle-encoded frames of a 70-sample-period signal decoded from the initial state by both decoders: within 2% (+0.5 of the oracle's integer step) on all 128 amplitudes (`enhanced_amplitudes_equal_the_oracle_decoders...`). Decoded level ratio ours/oracle 0.95 to 1.00, envelope correlation of the same frames through the two decoders 0.997 to 0.999 (they differ in the random phases of unvoiced synthesis: aligned SNR 1.2 to 4.5 dB, log-spectral distance about 6 dB). |
| 19 | **Cross-decoding.** | **Interoperable.** | Envelope correlation with the input over 300 frames: oracle encoder -> oracle decoder 0.902 to 0.941, oracle encoder -> our decoder 0.905 to 0.946, our encoder -> oracle decoder 0.914 to 0.949, our encoder -> our decoder 0.917 to 0.953 (the four files differ in how well any encoder tracks them; the decoders make no measurable difference). |

Edge signals (frame 30 of 55; `P2` is twice the initial pitch estimate; `b0`, `L`, `b2` as in the standard):

| signal | oracle `P2 / b0 / L / b2` | this crate | frames with equal `b0` (of 41) | equal `L` |
| --- | --- | --- | --- | --- |
| silence | 42 / 0 / 9 / 17 | 42 / 0 / 9 / 17 | 40 | 41 |
| sine 150 Hz | 214 / 173 / 49 / 22 | 214 / 174 / 49 / 21 | 0 | 41 |
| sine 440 Hz | 73 / 36 / 17 / 25 | 218 / 179 / 49 / 22 | 0 | 0 |
| sine 1000 Hz | 48 / 8 / 11 / 26 | 48 / 10 / 11 / 27 | 0 | 41 |
| sine 2000 Hz | 48 / 9 / 11 / 26 | 232 / 193 / 53 / 22 | 0 | 0 |
| sine 3500 Hz | 64 / 23 / 13 / 19 | 224 / 184 / 51 / 21 | 0 | 0 |
| DTMF 5 (770 + 1336 Hz) | 228 / 190 / 52 / 25 | 228 / 187 / 51 / 26 | 6 | 18 |
| DTMF 9 (852 + 1477 Hz) | 206 / 164 / 47 / 26 | 206 / 164 / 47 / 25 | 41 | 41 |
| white noise | 138 / 98 / 31 / 61 | 138 / 96 / 31 / 61 | 25 | 36 |
| white noise, 40 dB lower | 138 / 97 / 31 / 22 | 228 / 186 / 51 / 22 | 7 | 21 |
| pitch glide | 118 / 78 / 26 / 39 | 118 / 78 / 26 / 39 | 19 | 32 |

The 150 Hz case is the row 11 effect (period 53.33: the half-sample `b0` from the true period is 174, which is ours;
the oracle's eighth-sample estimate falls just below the boundary). The 440, 2000 and 3500 Hz cases are the
sub-multiple ambiguity of row 17.

### Damaged frames (Eq. 95-104, sections 7.7 and 7.8)

Oracle C cannot check this: it decodes bit vectors that are already error corrected and repeats only on `b_hat_0 > 207`
(as ours does). JMBE's source was read for the numbers only. Every number and rule is the same as ours and as
the PDF (page 62 read as an image): `epsilon_R = 0.95 epsilon_R(-1) + 0.000365 epsilon_T` (Eq. 96); repeat iff `b_hat_0`
invalid or (`epsilon_0 >= 2` and `epsilon_T >= 10 + 40 epsilon_R`) (Eq. 97-98); mute iff `epsilon_R > 0.0875` (7.8).
JMBE adds one rule the standard does not have: after more than three consecutive repeats it resets the parameters to
defaults and mutes. Ours has no repeat cap (a run of repeats raises `epsilon_R` through Eq. 96 until the 7.8 rule
mutes it); **not adopted, no divergence from the standard**. Ours also matches 7.8's comfort noise (uniform in
`[-5, 5]`, `synthesize_comfort_frame`). The first pass found mbelib's own thresholds differ from the standard's; JMBE
does not.

### Encoder speed

`PitchAnalysisFrame` tabulated `r(t)` per candidate and the look-ahead tracker re-evaluated its nested minimum per
candidate; the harmonic-band amplitude and the window transform `W_R(m)` were recomputed per bin. All are now tabulated
once per frame (or per process for `W_R`), with output byte-identical to before on 300 frames of speech. The
analysis went from about 19 ms per 20 ms frame to 0.33 ms (release).

### Changes in this pass

* `float::tia_102_baba::encoder`: `HighPassFilter`, `Encoder` filters its input (rounded to whole samples so that the
  float and fixed encoders see the same input); `FrameAnalysis::initial_pitch` and `Encoder::last_analysis` for
  diagnostics. `fixed::tia_102_baba::encoder`: integer `HighPassFilter`.
* `float` and `fixed` `pitch.rs`: `E(P)` floored at zero, ratio tests multiplied out; tabulated `r(t)`; the look-ahead
  nested minimum built bottom-up.
* `float::tia_102_baba::pitch_refinement`, `vuv`: tabulated `W_R`, twiddle table for the 256-point transform,
  per-harmonic amplitude cache (`SyntheticSpectrum`).
* `float` `mod.rs`: spectral amplitudes floored at 1.0 before the logarithm; the amplitude quantization split out as
  `quantize_spectral_amplitudes` (so a fixed set of amplitudes can be driven through it); `fixed::encode` floors too.
* Tests: `tests/ambe_tia_102_baba_oracle_encoder_validation.rs` (8 tests, numbers measured from the oracle); the
  float/fixed encoder parity thresholds in `tests/ambe_fixed_tia_102_baba_encode.rs` re-based on the new measurements
  (91.5% and 93.3% identical code vectors; the reasons are next to the assertions); a unit test pinning the exact
  limit cycle of a stationary tone now accepts any short exact cycle (10 frames, previously 2) because flooring the
  amplitudes changed the length but not the property.

Closing the first pass's open item on the look-ahead tracker: the sub-multiple rules (smallest sub-multiple tested first, thresholds 0.85 / 0.4 / 0.05, ratios 1.7 and 3.5, tested only when `P_hat_0 >= 42`, equivalently `P_hat_0 / 2 >= 21`) are identical to the oracle's. An independent reimplementation of the tracker reproduced both encoders' forward picks.

## What remains

* **D-STAR and AMBE+2 exposure (out of scope here, needs its own item).** Their encoders call the bare `FrameAnalyzer`, which has no Eq. 3 high-pass filter, so their pitch estimation has the DC-offset failure fixed here for TIA-102.BABA. Left untouched on purpose.

* **DVSI's own test vectors**, if Bruce has any. The chip cannot serve (it is a different codec).
* **JMBE** as a running second decoder: done (see Oracle D above).

* The oracle caps `E(P)` at 1 and refines to eighths of a sample; ours follows the standard on both (rows 9 and 11).
  If a hardware-compatible mode is wanted (interoperating frame for frame with a DVSI-derived encoder), those two,
  plus the oracle's 10-sample pitch-window stagger, are the choices to revisit; they change the bit stream but not
  interoperability, since every frame decodes.
* Voicing (row 12) is the one stage where the oracle and this crate genuinely differ in outcome, and the standard's
  text is the only arbiter available. A DVSI reference encoder would settle whether the 16-bit implementations'
  higher `D_k` is intended.
* The fixed-point tree has the quantizer and analysis parity tests against the float tree (see the parity tests) and
  now the same floors and filter; its own oracle comparison was not repeated.
