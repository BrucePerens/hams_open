# TIA-102.BABA (Project 25 full-rate IMBE) cross-validation findings

Scope: the floating-point implementation in `src/ambe/float/tia_102_baba/` (Project 25 Phase 1, 7200 bits per
second on the air, 4400 of them speech, 144 bits per 20 ms frame), checked against independent decoders. The
Digital Voice Systems (DVSI) chip cannot serve as an oracle: its `RATET(27)` / "P25 FEC" setting is a different
proprietary codec (see `AMBE_CHIP_VALIDATION_FINDINGS.md`). Date of this pass: 2026-09-19. No network access and
no chip were used.

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

## Still needs the oracles not yet obtained

* **OP25 `imbe_vocoder`** (Pavel Yazev): the only available encoder oracle. Needed for pitch estimation, voicing
  decision, spectral amplitude estimation, gain and higher-order quantization, and bit-exact cross-decoding of
  each other's frames. Nothing in this pass validated our encoder's analysis half, only its quantization and framing
  (through the decoders above).
* **JMBE** (Java, LGPL or GPL to be checked): second decoder, and the only one covering AMBE variants.
* DVSI test vectors, if Bruce has any.
* Not yet examined: look-ahead pitch tracker sub-multiple and negative-error handling, encoder speed (about
  19 ms per 20 ms frame), damaged-frame policy (Eq. 97-104, section 7.8) against mbelib's different repeat and mute
  rules, and edge signals (silence, tones).
* The fixed-point tree has the same wire-layer change and shares the tables; it still lacks `encode_frame` and
  quantization parity.
