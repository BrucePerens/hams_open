# AMBE chip validation: findings so far

Real DVSI AMBE3003 hardware (a DVstick-33) is now reachable on the LAN via `AMBEServer3003`
(`192.168.10.189`, UDP ports 2460-2462, one emulated AMBE3000 channel per port), giving this
codebase's own from-spec P25 AMBE codec (`src/ambe/`) a real ground truth to validate against, and
the concrete configuration data needed to build a D-STAR mode. This document records what's been
confirmed so far, what's still open, and exactly how to reproduce or continue the validation.

## 1. The chip's own rate table, confirmed against DVSI's own manual

DVSI's official "USB-3000 Manual" (downloaded from https://www.dvsinc.com/dlapps/appsoft.shtml)
gives the real, complete `RATET` index table for the AMBE-3000-generation chip (which is what's
inside a USB-3000/3003/3012 despite the table's own "AMBE-1000/2000/3000 Rates" section headers --
those are backward-compatible rate *definitions* the chip still implements, not evidence of which
chip generation is actually inside it; the manual states directly that USB-3000/3003 use the
AMBE-3000/AMBE+2 chip). Cross-checked index-for-index against `github.com/janakj/ambe`'s own
independently-sourced table -- exact agreement for indices 0-61 (janakj's table doesn't cover 62-63,
which are satellite-only rates).

**Index 27 -- 7200 total / 4400 speech / 2800 FEC bps -- is this codebase's own codec, exactly.**
`88 + 56 = 144` bits (this codec's own `VOICE_BITS`/`FEC_BITS`), and `4400 * 0.02 = 88`,
`2800 * 0.02 = 56` bits per 20ms frame -- an exact match, not a coincidence. Confirmed live: sending
`RATET(27)` to the real chip and encoding a test frame returns exactly 144 bits every time.

## 2. A real correction to this document's own prior citation: BABA vs. BABA-1

`AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md` and this crate's own doc comments (`ambe/mod.rs`,
`ambe/pitch.rs`) cite "TIA-102.BABA" and describe it in prose as the "half-rate"/"AMBE-2000-era"
vocoder. Independently verified against the actual document titles (not assumed): **TIA-102.BABA
itself is titled "Project 25 IMBE Vocoder Description" -- the original, full-rate IMBE spec.**
**TIA-102.BABA-1** (a distinct addendum document, not the same as BABA) is titled "APCO Project 25
Half-Rate Vocoder Addendum." Given this codec's own frame structure (144 bits, four `[23,12]` Golay
plus three `[15,11]` Hamming codes, matching the well-documented classic P25 Phase 1 IMBE FEC layout)
and its exact numeric match to RATET index 27's "AMBE-2000 Rates"-labeled 7200/4400/2800 split, this
codec is almost certainly a real, correct implementation of the **original full-rate IMBE algorithm
(TIA-102.BABA itself)**, not the half-rate addendum the surrounding prose has been calling it. The
actual equations transcribed into `tables.rs`/`pitch.rs`/etc. should still be double-checked against
the real BABA PDF text directly (this correction is about the document's *identity*, not yet a
line-by-line re-verification of every transcribed table against it) -- but the prose describing this
codec as "half-rate"/"AMBE-2000" throughout the existing docs is now known to be imprecise and should
be corrected once that direct check is done.

## 3. Live validation harness: built, working, and already surfacing a real problem

`examples/ambe_chip_validate_p25.rs` configures the real chip for `RATET(27)`, feeds it a stationary
8kHz test tone 160 samples at a time over the real AMBEServer3003 UDP link, and runs the identical
signal through this crate's own encoder (bypassing the pitch tracker deliberately, using the tone's
exact analytically-known pitch, to isolate the quantization/DCT/FEC tables from any separate pitch-
tracker bug). Run it with `cargo run --example ambe_chip_validate_p25 -- 192.168.10.189 2460`.

**First real result, not yet resolved:**

- The real chip's own output is **bit-identical across all 40 test frames** once the signal is
  running -- expected and correct for a perfectly periodic input (a 200Hz tone at 8kHz repeats every
  exactly 40 samples, so every 160-sample frame is byte-identical PCM; a well-behaved closed-loop
  encoder should converge to a fixed output).
- **This crate's own encoder does not converge to a fixed output for the same perfectly periodic
  input.** It instead cycles through several distinct 144-bit values with a period of exactly 12
  frames (`frame 8 == frame 20 == frame 32`, etc.) -- a real anomaly worth its own investigation
  before anything else, independent of chip comparison: a closed-loop predictive encoder's internal
  state (`FrameState`'s `xi_max` energy tracker, Eq. 41; the prediction-residual feedback, Eq. 54)
  should settle toward a fixed point for a genuinely stationary input, not an oscillating limit
  cycle. This needs to be understood and fixed (or shown to be spec-correct behavior, if it turns out
  the equations themselves produce a genuine multi-frame periodicity for degenerate-stationary input)
  before a meaningful bit-exact comparison against the chip is possible at all.
- Bit agreement between the two streams across the offset search is essentially at chance
  (~49%, every tested frame-alignment offset from -4 to +4), which is far too low to be "close but a
  few table entries differ" -- this is more consistent with a real structural mismatch (bit-packing
  order, field order, or the modulation/scrambling step) than with a subtle numeric table error, and
  should be diagnosed *before* trusting any specific-table-value comparison. A concrete next
  diagnostic, not yet done: feed the chip's own real output frames into this crate's own
  `decode::DecoderState::decode_frame` and see whether the FEC (Golay/Hamming) checks pass at all --
  if they don't, that's strong evidence the bit-packing/field-order assumption
  (`pack_frame_msb_first` in the harness: c0..c7 concatenated MSB-first, widths
  `23,23,23,23,15,15,15,7`) is simply wrong and needs to be found empirically rather than assumed.

**Update, same session, after step 2 below**: tried this immediately (`examples/ambe_frame_diagnose.rs`,
also committed) -- unpacked the chip's real captured frame under five plausible bit/byte/field-order
conventions (MSB-first, LSB-first-per-byte, whole-stream-reversed, reversed field order, reversed byte
order) and ran each through this crate's own `decode_parameters`. **All five fail identically** (every
one triggers section 7.7's frame-repeat condition, meaning none produces a clean Golay decode of
`c_hat_0`). This rules out a simple bit-transposition bug as the explanation and points at something
more fundamental -- most likely that DVSI's chip, even at an aggregate bitrate matching RATET index
27 exactly, does not necessarily use the *same* Golay/Hamming split, bit-prioritization order, or
modulation PRN that TIA-102.BABA's own text describes; DVSI's own USB-3000 Manual never confirms the
chip's internal bitstream layout is identical to the published spec's example implementation, only
that the aggregate bit budget matches.

**A real, related discovery while chasing this**: G4KLX's AMBETools ships a full IMBE FEC codec
(`Common/IMBEFEC.cpp`) for converting to/from the P25 *over-the-air* dibit/`.dvtool` format --
real, working code showing an actual **144-bit interleave permutation** (`IMBE_INTERLEAVE[144]`) is
applied on top of the raw `c0..c7` concatenation before transmission, plus the same
content-dependent whitening/PRN construction this codec's own `modulation.rs` implements (seeded from
`c0`'s own 12 data bits via a linear congruential generator: `p = 16*c0; p = (173*p+13849) mod 65536`
-- worth directly comparing against `modulation::pseudo_random_sequence`'s own formula for an exact
match, not yet done). **This interleave is very likely an RF-modem/channel-symbol-level step applied
*downstream* of the vocoder chip, not something the AMBE3003 chip itself does to its own USB/serial
`CHANNEL` packets** -- AMBETools uses `CIMBEFEC` specifically for its `.dvtool`/over-the-air file
format conversion, separately from `DV3000SerialController`'s own direct chip I/O, which never
interleaves. So this probably isn't the missing piece for comparing against the chip's raw serial
output directly, but it's the right place to look once channel-frame-level work (a real, separate,
already-disclosed scope boundary per `mod.rs`'s own doc comment) is in scope.

**Update, same session, after the pitch-perturbation test below**: captured the chip's real output at
six nearby tones (150/175/200/225/250/300 Hz) and XORed each against the 200Hz baseline. The bit
positions that actually change for the four close frequencies (150/225/250, all within ~7-12 bits of
200's frame) cluster in a clean arithmetic sequence with **stride exactly 12** (e.g. `15, 27, 39, 51,
63, 75, 87, 99, 123, 135`) -- a real, structural signal, not noise, and consistent with some kind of
block-interleave with a 12-wide period (`144 / 12 = 12`). Tried deinterleaving the captured frame
through **eight** different concrete hypotheses before decoding: this crate's own real Annex H
dibit-interleave table, a real working reference implementation's own interleave table (AMBETools'
`IMBE_INTERLEAVE[144]`, tried both bit-numbering conventions), plus the five bit/byte/field-order
permutations from the previous update. **All eight still fail.** Checked precisely what "fail" means
here, since it matters: `golay_decode`'s own brute-force nearest-codeword search means `epsilon_0`
can never exceed 3 for *any* input at all -- Golay(23,12,7) is a **perfect code** (covering radius
equals packing radius, exactly 3), so literally every possible 23-bit pattern sits within distance 3
of some codeword. Every single one of the eight hypotheses above scored exactly `epsilon_0 = 3`, the
theoretical maximum -- meaning none of them found real structure; this isn't "close but not quite,"
it's indistinguishable from feeding the decoder pure noise.

**Honest assessment before continuing further**: the 300Hz frame's own huge bit-count jump (76 of
144 bits differ from 200Hz, versus single digits for the four closer frequencies) shows the chip's
real output *is* behaving like a genuine, well-structured vocoder -- pitch changes cause small,
localized differences until a harmonic-count-bucket boundary is crossed, then a large structural
change, exactly as this codec's own `L_hat`-dependent bit allocation would predict. So the chip is
producing real, meaningful, non-random output; the problem is specifically that this codec's own
assumed field/FEC layout (four Golay(23,12) + three Hamming(15,11) + 7 raw, in `c0..c7` order) has not
yet been matched to whatever layout DVSI's chip actually uses at this rate, despite the aggregate bit
budget being an exact, independently-confirmed match. The stride-12 clue is real and worth pursuing
further, but cracking DVSI's undocumented proprietary bit layout from here on is a genuine, patient
reverse-engineering project -- closer in kind to what the mbelib community did for D-STAR/DMR/NXDN
over real calendar time via captured-traffic analysis, not something a few more guesses will resolve.
A systematic, automated search (score every plausible field-order/bit-direction/rotation combination
against `epsilon_0` across *many* captured frames at once, to avoid a lucky-looking low score on just
one frame) is the right next tool if this is worth continuing, rather than further hand-picked
hypotheses.

**Real next steps, in order:**
1. **A more powerful empirical technique than guessing bit-order permutations further**: capture the
   chip's real output for *two slightly different* stationary tones (e.g. 200Hz and 220Hz) and XOR the
   two 144-bit frames. Since pitch is the parameter most directly controlled by tone frequency, the
   bit positions that actually change reveal, empirically and with no assumptions about DVSI's
   internal layout, exactly where pitch information lives in the chip's own real output -- then that
   observed pattern can be compared against where this crate's own `bit_prioritization`/`quantize_fundamental_frequency`
   believe pitch lives, which is a much stronger diagnostic than continuing to guess whole-frame
   bit/byte orderings blindly.
2. Diagnose and fix the 12-frame oscillation on a stationary input, independent of the chip
   comparison -- a closed-loop predictive encoder should converge to a fixed point for a genuinely
   stationary input, not cycle.
3. Directly compare `modulation::pseudo_random_sequence`'s own LCG formula against the
   `173*p + 13849 mod 65536` construction found in AMBETools' `IMBEFEC.cpp` above -- if this codec's
   own transcription of the spec's PRN differs from that real, working reference, that's a concrete,
   fixable bug, independent of the interleave/framing question.
4. Extend the harness with a real decode-direction test (feed known bits to the chip's decoder, i.e.
   send a `CHANNEL` packet and read back the `SPEECH` response, compare PCM against this crate's own
   `decode_frame` output) once the encode-direction framing question above is resolved.

## 4. D-STAR: real, primary-source data now in hand, implementation not yet started

D-STAR's on-chip configuration is confirmed from **two independent primary sources**: DVSI's own
USB-3000 Manual (Table 30, "Custom Rate Control Words," explicitly labeled "interoperable with
D-STAR") and G4KLX's AMBETools source (`DV3000_REQ_DSTAR_FEC`) -- both give the exact same 6-word
RATEP rate-control-word: `0x0130 0x0763 0x4000 0x0000 0x0000 0x0048`, decoding (per DVSI's own manual)
to **3600 total / 2400 speech / 1200 FEC bps** -- i.e. a 72-bit, 9-byte frame every 20ms, not this
codec's own 144-bit frame. D-STAR's rate does not correspond to any single `RATET` table index; the
chip must be given these literal RCW words directly. A no-FEC D-STAR variant (`RATET` index 0, 2400
speech bits only, no FEC) is also documented and can be used to isolate FEC-layer bugs from
speech-quantization bugs during D-STAR development.

D-STAR's actual bit allocation (not documented in any TIA standard DVSI ships) was reverse-derived
from `mbelib` (github.com/szechyjs/mbelib) -- confirmed ISC-licensed for its D-STAR files
(`ambe3600x2400.c`/`ambe3600x2400_const.h`, permissive, not GPL as initially assumed), and per this
project's own established position (`docs/AMBE_TABLE_COPYRIGHTABILITY_ANALYSIS.md` in `hams_com`),
extracting the *numeric table values* for cross-reference is fine regardless -- new Rust code is
still written fresh, not adapted from mbelib's own C source.

**D-STAR's real 72-bit frame structure**, verified directly from mbelib's source:

- Four sub-blocks: `C0` (24 bits) + `C1` (23 bits) + `C2` (11 bits) + `C3` (14 bits) = 72 bits total.
- FEC: **only Golay(23,12)** is used (no Hamming, unlike this codec's own P25 FEC layout) -- one
  Golay(23,12) codeword each for `C0` (using 23 of its 24 bits, the 24th being an unverified spare/
  parity bit mbelib itself never actually checks) and `C1`. `C2` and `C3` are carried completely
  unprotected. Net: `12 + 12 = 24` data bits recovered from FEC-protected fields, `11 + 14 = 25` more
  carried raw, for **49 total decoded parameter bits** (72 - 49 = 23 bits of real FEC overhead).
- **A real quirk with no equivalent in this codec's own P25 path**: before decoding, `C1` is XORed
  with a pseudo-random sequence seeded from `C0`'s own already-corrected pitch bits (a
  content-dependent descrambling step, not the fixed/positional modulation this codec's own
  `modulation.rs` implements for P25) -- this needs its own from-spec (or, failing an available
  spec, carefully-referenced-and-independently-tested) implementation, not a copy of mbelib's own
  scrambling code.
- Parameter groups (bit widths, and which of `C0`-`C3` each comes from) are fully mapped out --
  pitch/tone index (7 bits), voicing pattern (4 bits), a gain term (6 bits), two PRBA
  (spectral-shape) gain groups (9 and 7 bits), and four higher-order-coefficient blocks (4 bits
  each) -- see the research transcript for the full bit-by-bit table if picking this up later; the
  actual lookup-table *contents* (pitch table, VUV pattern table, gain tables, PRBA tables, HOC
  tables) still need to be pulled from mbelib's source into this crate's own `tables.rs`-equivalent
  for D-STAR before any encoder/decoder can be written against them.

**Not yet done, and the largest remaining piece of this whole task**: writing the actual D-STAR
encoder/decoder module (a new `src/ambe_dstar/` or a D-STAR-specific path inside `src/ambe/`,
following this crate's own established pattern of one well-tested, doc-commented module per pipeline
stage), pulling in the real mbelib table *values* (not code), implementing the Golay-only FEC and the
C1 descrambling step, and validating it against the chip via the confirmed D-STAR RCW the same way
`ambe_chip_validate_p25.rs` already does for the P25 path.

## 5. D-STAR live validation: frame size, interleave, and framing confirmed against 320 real frames

Per Bruce's own direction ("do D-STAR first, then P25 -- maybe we will learn something from how
AMBE works for D-STAR"), the `ambe_dstar` module (see its own doc comment for the full frame
structure and mbelib provenance) was built and validated against the live chip before returning to
the P25 mystery above. Two real things were learned that generalize back to P25:

- **The same content-dependent PRN whitening construction (LCG: multiplier 173, increment 13849,
  modulus 65536) is used by both generations** -- confirmed independently in mbelib's real D-STAR
  source and in AMBETools' real P25 IMBE source (`Common/IMBEFEC.cpp`, found during the P25
  investigation above). This is genuine, useful cross-generation evidence for the DVSI-family
  whitening scheme as a whole, not something to re-derive per mode.
- **D-STAR's Golay(23,12,7) code is the exact same perfect code the P25 investigation already found
  ambiguous to validate with** (confirmed by reusing `super::ambe::fec::golay_encode`/`golay_decode`
  directly, without needing a second implementation) -- so the same "a low corrected-error count
  isn't by itself proof of correct framing" caveat applies here too, documented directly in
  `ambe_dstar::decode`'s own doc comment (after catching and fixing a first draft that had this
  backwards).

**Live validation, using the confirmed D-STAR RATEP configuration** (`0x0130 0x0763 0x4000 0x0000
0x0000 0x0048`, `examples/ambe_dstar_chip_check.rs`):

- Configuring the real chip with this exact RCW and encoding a test tone returns **exactly 72 bits
  (9 bytes)** every time -- confirms the frame *size* is right, live, not just on paper.
- **Real confound found and worked around**: the first perturbation tests used tone frequencies
  (150/175/225/300 Hz) whose period does not evenly divide the 160-sample frame, so consecutive
  "steady-state" frames of the same tone were not actually bit-identical (inter-frame phase drift).
  Fixed by restricting to frequencies whose period divides 160 exactly (50/100/200/250/400/500/800/
  1000 Hz) with frame-relative (not continuous) phase.
- **Second real confound found**: the chip's own D-STAR-mode encoder was found to **never converge to
  a truly fixed steady-state output** on a stationary pure-tone input -- 40 consecutive frames at a
  fixed 100 Hz tone never repeat exactly, oscillating quasi-periodically instead (this is plausibly
  the vocoder's pitch tracker hunting on a stimulus with no natural voice-like formant structure, not
  a bug in this investigation's own tooling). At 200 Hz and above the chip's output *does* converge
  to a fixed value after a short settling period (confirmed: standard deviation of decoded `f0`
  across 40 captured frames drops from the microvolt-noise floor of floating point at 200-1000 Hz,
  down from real double-digit-Hz variance at 50-100 Hz) -- worth knowing for anyone else probing this
  chip's D-STAR mode with synthetic test tones.
- **The real bit-order/framing bugs, found and fixed**: an initial correlation-based bit-order search
  (decoded `f0` vs. true test-tone frequency across byte-order x per-byte-bit-order x field-order
  hypotheses) never rose above noise-level correlation (~0.27-0.86 out of a possible 1.0, no clear
  winner) for any hypothesis -- misleading, because D-STAR's Golay(23,12,7) code is a genuine
  *perfect* code (covering radius = packing radius = 3), so a **falsification test** (does a
  hypothesis Golay-decode real, noise-free chip output with *zero* corrected errors on essentially
  every captured frame, not just "some" or "fewer than others") is the right diagnostic, not
  correlation. Two real, independent framing bugs were found and fixed this way, both now corrected
  in `ambe_dstar::interleave`/`ambe_dstar::decode`/`ambe_dstar::encode`:
  1. **The 9 raw CHAND bytes are not a simple `C0||C1||C2||C3` concatenation.** They carry the exact
     same block-interleave D-STAR uses over the air -- confirmed against `szechyjs/dsd`'s real,
     working GMSK demodulator (`include/dstar_const.h`'s `dW`/`dX` tables, fetched and verified
     directly, not transcribed from memory). This means DVSI's chip apparently transmits/receives a
     D-STAR CHAND frame pre-interleaved in exactly the RF transmission order, letting a repeater
     relay CHAND bits to/from RF with no separate interleave step of its own -- a genuinely
     interesting, previously-undocumented (in this codebase) design fact about the chip. Each byte's
     bits are read LSB-first.
  2. **`C0`'s spare bit is its own LSB, not its MSB.** This crate's first version of `parse_frame`
     masked off `C0`'s top bit as the spare and kept the bottom 23 bits as the codeword -- backwards
     from mbelib's real convention (`mbe_eccAmbe3600x2400C0`: `in[j] = ambe_fr[0][j+1]`, i.e.
     `ambe_fr[0][0]` is the spare and `ambe_fr[0][23..1]`, MSB-first, is the codeword), confirmed by
     fetching and reading mbelib's real `ambe3600x2400.c` directly. The wrong version's shifted-by-one
     codeword happened to still Golay-decode "successfully" (zero corrected errors) on some frames
     purely by coincidence -- to the *wrong* 12-bit data value -- which then poisoned every `C1`
     whitening seed downstream, making `C1` decode with the maximum possible corrected-error count
     (3) on literally every single frame, a strong enough signature to localize the bug via a direct,
     line-by-line cross-check against a from-scratch transliteration of mbelib's real C algorithm.
- **Final result, confirmed live against the real chip** (`examples/ambe_chip_validate_dstar.rs`,
  the permanent committed harness replacing the throwaway Python capture script): **320/320 real
  captured frames (50/100/200/250/400/500/800/1000 Hz, 40 frames each after a 10-frame settling
  discard) Golay-decode with zero corrected errors on both `C0` and `C1`** -- including the
  above-vocal-range 500/800/1000 Hz tones and the never-fully-converging 50 Hz tone (every individual
  frame it produces, even mid-oscillation, is still a well-formed, validly-encoded D-STAR AMBE
  frame -- the earlier "doesn't converge" observation was never evidence of corruption, just of the
  chip's pitch tracker legitimately changing its answer frame to frame). This is the real, decisive
  confirmation the perfect-code caveat above says a single frame's low error count alone can never
  give: 320 independent frames all landing on zero error is (1/2048)^320-level evidence against
  chance, not a coincidence.
- Semantic validation (does decoded `f0` actually track true input frequency, not just "does FEC
  validate") is the next real step now that framing is confirmed -- worth doing with a
  harmonic-rich stimulus (sawtooth/pulse train) rather than a pure sine, since AMBE's pitch estimator
  does harmonic matching and a single sinusoid is octave-ambiguous by construction; a pure tone can
  alias to a harmonic or subharmonic of its own true period even under fully correct framing.

## 6. Reproducing this work

- AMBEServer3003 runs as a systemd service on `pi500-1` (`192.168.10.189`), already configured and
  enabled at boot. **Never trigger this specific chip's UART BREAK/hard-reset** -- confirmed this
  session that it reliably locks up the chip's command pipeline until a full reboot; AMBEServer3003
  itself never does this, and neither should any future test code.
- `cargo run --example ambe_chip_validate_p25 -- 192.168.10.189 2460` from
  `hams_open/daemons/ham_digital_modes/` reproduces the P25-path finding above.
- `cargo run --release --example ambe_chip_validate_dstar -- 192.168.10.189:2460` runs the full,
  permanent live validation harness against the real chip: configures D-STAR RATEP, captures 40
  frames each at 8 test frequencies, and reports the Golay-decode exact-match rate for each (expect
  320/320 total). This is the committed replacement for the throwaway Python capture script this
  investigation started with.
- `cargo run --example ambe_dstar_chip_check -- <18-hex-char frame>` decodes one real captured 9-byte
  D-STAR frame (chip wire order) through `ambe_dstar::interleave` + `ambe_dstar::decode` and prints
  the recovered parameters -- useful for inspecting a single frame by hand.
- The mbelib tables themselves (pitch/VUV/gain/PRBA/HOC arrays) are already pulled into this repo
  directly, in `src/ambe_dstar/tables.rs`/`tables_prba.rs`/`tables_hoc.rs` -- generated
  programmatically from mbelib's own C header (not hand-transcribed) to avoid transcription error;
  this caught and fixed two real bugs during that process (see that commit's own message for detail).
