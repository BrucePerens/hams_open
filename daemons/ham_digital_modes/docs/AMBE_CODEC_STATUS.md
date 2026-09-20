# AMBE and IMBE codecs in `ham_digital_modes`: status and evidence

Last updated 2026-09-19. Everything below was measured; the commands that reproduce each number are given.

## Modes and features

| Mode | Where | Encode | Decode | Tones | Notes |
|---|---|---|---|---|---|
| D-STAR (AMBE 2400 with 1200 FEC, 72-bit frames) | `src/ambe/{float,fixed}/dstar` | float and fixed | float and fixed | 16 DTMF digits, single tones 200 Hz-3.8 kHz, tone level (volume) field | conforms to the DVSI chip's decoder: envelope correlation 0.995-0.998 |
| AMBE+2 half rate (DMR, System Fusion, NXDN, P25 Phase 2; feature `ambe_plus_2`) | `src/ambe/{float,fixed}/ambe_plus_2` | float and fixed | float and fixed | DTMF, single tones and call-progress tones (dial, ring, busy) | decoder envelope correlation with the chip 0.966-0.986; needs the Cargo feature `ambe_plus_2` (patent gating, see `AMBE_PLUS_2_NOTES.md`) |
| TIA-102.BABA IMBE full rate (P25 Phase 1, 144-bit frames) | `src/ambe/{float,fixed}/tia_102_baba` | float and fixed | float and fixed | none (not part of the standard) | standard wire layer; cross-validated against mbelib, `imbe.rs` and OP25 `imbe_vocoder` |
| Chip framing at `RATET(27)` / "P25 FEC" | `src/ambe/dvsi_p25fec` | frame layout, DTMF and DTX signalling only | | | a different DVSI codec, not IMBE; documented, not a full codec |

Common to all: streaming encoders (`push_samples`, `next_frame`, `finish`) that emit tone frames for detected tones and never
report voiced speech as a tone; decoders with damaged-frame handling; float and fixed-point trees with the same behaviour
(fixed-point uses no floating point at all: `tests/ambe_fixed_no_float_tokens.rs`).

## Damaged frames: normal operation versus chip-conformance test mode

* **The default is `ErrorPolicy::Concealing`**, a policy designed for audio quality (`src/ambe/float/concealment.rs` and its fixed-point
  port), not for compatibility with any reference decoder. On a stream with no channel errors it decodes bit-for-bit like the plain
  decoder. When errors appear it (1) estimates the channel error rate from how many errors the Golay codes corrected, (2) for the raw
  (unprotected) bits that matter most (in D-STAR the voicing pattern, low gain bits, low spectral-vector bits and the pitch's lowest
  bit) considers each single-bit correction and picks the reading that best continues the previous frame (level, spectral shape,
  voicing and pitch, with priors measured on real speech), paying a cost per flip that depends on the estimated error rate, and (3)
  if even the best reading is implausible, repeats the previous frame with a fade (halving each frame) instead of muting, keeping the
  predictor state, and resets only after 30 repeated frames. Which bits matter was measured (`examples/ambe_bit_sensitivity.rs`:
  the top gain bits, the voicing pattern and the top pitch bits cost the most; the low higher-order coefficient bits almost nothing).
* Measured with `examples/ambe_error_concealment_eval.rs` (real speech, random bit errors in the 72-bit wire frames, distance of the
  spectral envelope from the error-free decode, D-STAR; the constants were tuned on one set of error patterns and confirmed on others
  and on AMBE+2, where the same policy also beats both alternatives):

| channel | clean policy (mbelib) | concealing (default) |
|---|---|---|
| 0.5% bit errors | 0.76 dB | 0.57 dB |
| 1% | 1.02 dB | 0.99 dB |
| 2% | 1.77 dB | 1.65 dB |
| 5% | 4.98 dB (0.2% muted) | 3.65 dB |
| 10% | 26.9 dB (24% of active frames muted) | 8.3 dB (2% muted) |
| bursty, 3% average | 9.45 dB (8% muted) | 3.99 dB (1.3% muted) |

* `ErrorPolicy::Clean` is mbelib's policy (repeat three frames, then mute), kept for reference and tests.
* `ErrorPolicy::ChipCompatible` reproduces the chip (repeats when the first Golay block corrected 3 errors, never mutes, so
  garbage frames produce loud bursts); it is a defect of the chip and exists only as an opt-in for conformance tests. It is
  also selectable on all four synthesis decoders (float and fixed, D-STAR and AMBE+2).
* TIA-102.BABA keeps the standard's own repeat and mute rules (sections 7.7 and 7.8). Its stronger error correction (four Golay
  and three Hamming code vectors) leaves little to conceal: measured with `examples/ambe_error_concealment_eval_tia.rs`, the
  standard's policy is within 0.05 dB of a fading repeat up to 5% bit errors. At 10% and in bursts a fade lowers loudness
  excursions (worst 1%: 13.4 to 6.7 dB in bursts) but raises the envelope distance (2.90 to 3.68 dB) and mutes 4.6% of active
  frames, so it is not a clear win and was not adopted.
* Chip conventions that would lower quality (its pitch index on unvoiced frames, delayed mute on reserved pitch codes) are
  deliberately not copied.

## Objective quality (encode real speech, decode, compare with the input)

`cargo run --release --features ambe_plus_2 --example ambe_roundtrip_quality_report` (600 frames of each of four speakers of the
Open Speech Repository fixtures). Energy correlation is of log frame energy after aligning the codec's delay; spectral distance
is the mean over active frames of the root-mean-square difference of log spectra (100-3800 Hz); level offset is the mean output
level minus input level on active frames. The chip's own round trip has a similar offset (about -4 dB on two speakers; noise
substitution in unvoiced segments), so a negative offset is inherent to this codec family.

| codec | energy correlation | spectral distance (dB) | level offset (dB) |
|---|---|---|---|
| D-STAR float | 0.9375 | 9.51 | -4.20 |
| D-STAR fixed | 0.9376 | 9.50 | -4.19 |
| AMBE+2 float | 0.9368 | 9.02 | -3.98 |
| AMBE+2 fixed | 0.9368 | 9.01 | -3.97 |
| TIA-102.BABA float | 0.9390 | 9.47 | -3.74 |
| TIA-102.BABA fixed | 0.9390 | 9.47 | -3.74 |

Fixed and float agree to the fourth decimal in every mode.

Real-time margin (release build, one core, per 20 ms frame): D-STAR float encode 0.92 ms and decode 0.27 ms, fixed encode 0.38 ms and
decode 0.41 ms; TIA-102.BABA float encode 0.4-1.0 ms and decode 0.43 ms. Every encoder and decoder uses under 5% of one core.

## Agreement with the DVSI chip (tested against the real AMBE-3000R)

| Measure | Result |
|---|---|
| D-STAR decoder envelope correlation, four speakers | 0.995-0.998 (was 0.84-0.94 with mbelib's formulas) |
| AMBE+2 decoder envelope correlation, four speakers | 0.966-0.986 |
| D-STAR encoder through the chip's decoder (aligned) | 0.989, equal to our own decoder |
| AMBE+2 encoder through the chip's decoder (aligned) | 0.973 (the chip's own encoder: 0.76) |
| Fixed versus float decoders on 3,320 real chip frames, relative amplitude error | 0.18% |
| Pitch maps | D-STAR fitted to 0.2% (log-linear, 45.94 steps per octave); AMBE+2 table matches to 0.3% |
| Quantizer tables | every field's response slope near 1 and correlation above 0.9 (both modes) |

What was different from mbelib in D-STAR (the amplitude predictor weight 0.8, the gain `2*DG` with no memory, the harmonic count,
the pitch map) is documented in `src/ambe/float/dstar/mod.rs`. The scans that found it are the examples
`dstar_field_scan` and `ambe_plus_2_field_scan` (modes `f0scan`, `lscan`, `rho`, `ljump`, `errs`, `errclass`, ...).

## Agreement with independent implementations of the standard (TIA-102.BABA)

`docs/references/tia_102_baba_cross_validation.md`: decoder against mbelib 1.3.0 and `imbe.rs` (1,200 speech and 8,649 sweep
frames agree in every parameter), encoder against OP25 `imbe_vocoder` (bit-exact prioritization and packing on 2,880 frames;
several real bugs found and fixed: default wire layer, missing input high-pass filter, error-function and amplitude floors).
JMBE was read for its damaged-frame policy but not built.

## Level match to the chip (D-STAR and AMBE+2)

Measured on the live chip with `ambe_chip_pcm_vs_float_synthesis_half_rate` over four real speakers (about 600 active frames per
mode), frames grouped by spectral tilt, chip level minus ours in three bands (100-1000, 1000-2000, 2000-3800 Hz). The standard's
synthesis was 1.1 to 1.7 dB louder than the chip on noise-like frames and about 1 dB louder in the top band on every kind of frame.
Two constants in `float/mbe_synthesis.rs` (mirrored in the fixed-point tree) were fitted to remove that: the high-frequency lift
weight 0.12 to 0.10 (its earlier value was fitted on synthetic harmonics) and a new gain of 0.87 on the unvoiced half only. The root-mean-square
class-mean level error fell from 0.98 dB to 0.33 dB and the frame-energy correlation rose slightly (D-STAR 0.995 to 0.998, AMBE+2 0.978 to 0.990).
The standard TIA-102.BABA decoder is unchanged (unvoiced gain 1.0).

## Known open items (all low impact)

* The chip's frame crossfade and noise source differ from the standard's (see `docs/references/AMBE_CHIP_NOISE_GENERATOR.md`: its
  noise generator has period 65,536 and is not identified), so unvoiced output cannot match the chip sample for sample.
* Chip-compatible repeats now run the AMBE+2 gain recursion on the damaged frame (measured: the frames after a repeat decay as the
  recursion's 0.5 memory predicts). Still not modelled: the chip's first frame after a repeat is louder than ours (+11.5 dB against +7.1 dB
  in the probe) and each further repeat on the chip rises by about 1.5 dB; test mode only.
* AMBE+2 frame-energy correlation with the chip (0.978-0.990) is limited by fully unvoiced frames, where the two decoders' noise is
  different and the frame energy differs by 2-3 dB standard deviation; the fine-scale (64-sample) log-envelope correlation is 0.988 with no
  timing offset. Unvoiced frames with many harmonics still come out about 1.7 dB louder than the chip's in the 100-500 Hz band.
* Reserved D-STAR pitch codes 125 and 127 are invalid on the chip; normal operation decodes them leniently (a tone frame with a flipped uncoded bit is better decoded), and the chip-compatible policy reproduces the chip.
* No DVSI test vectors were available; JMBE was not run.

## Where things are

* Tools: `examples/*chip*` (need the chip's UDP bridge), `examples/ambe_roundtrip_quality_report.rs` (no chip),
  `examples/tia_102_baba_*` (cross-validation dumps).
* Findings and history: `docs/references/AMBE_CHIP_VALIDATION_FINDINGS.md`, `AMBE_CHIP_NOISE_GENERATOR.md`,
  `tia_102_baba_cross_validation.md`; working notes in `hams_com/night_shift_todo/high/ambe-float-completion-then-bughunt-then-fixed-parity-3c8f1d2e.md`.
* Test suite: `cargo test --release --all-features --no-fail-fast` (about 700 tests), `cargo clippy --release --all-targets --all-features`.
