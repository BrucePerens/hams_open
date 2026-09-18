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
  running. Originally read here as "expected and correct for a perfectly periodic input, since a
  well-behaved closed-loop encoder should converge to a fixed output" -- **that expectation itself
  was wrong, corrected after direct investigation below**, and in any case the chip's own
  convergence turns out to be weak evidence either way: item 2's own findings (already in this same
  document, below) show the chip's real output has no recognizable TIA-102.BABA FEC structure under
  eight independently-tried bit-layout hypotheses, strong evidence it's running a materially
  different vocoder generation (most likely DVSI's own AMBE+2) at this rate, not the published IMBE
  algorithm this module implements. A different algorithm's convergence behavior on this stimulus
  says nothing about whether TIA-102.BABA's own Eq. 54/62/63 are supposed to converge here.
- **This crate's own encoder does not converge to a fixed output for the same perfectly periodic
  input -- diagnosed directly, not just observed, and this is real DPCM limit-cycle behavior, not a
  bug.** Traced with a purely in-process reproduction (no chip needed): feeding the identical
  200Hz-tone PCM through `encode_frame`/`FrameState` repeatedly showed the frame-to-frame
  *unquantized* spectral amplitude estimate is genuinely bit-identical (confirmed to ~10 significant
  digits), so the oscillation is entirely internal to the prediction/quantization/reconstruction
  loop, not an artifact of the test harness's own windowing. The real mechanism: the gain vector's
  second-stage DCT coefficient `G_hat_2` (Annex F, 6 bits at this tone's `L_hat=18`) lands on a
  quantizer bin boundary, and `prediction::prediction_coefficient`'s own `rho ~= 0.49` feedback term
  (Eq. 54/77) amplifies each frame's quantization error by `1/(1-rho) ~= 1.96` before feeding it back
  into the next frame's residual -- the textbook condition for a closed-loop quantized DPCM predictor
  to settle into a stable period-2 limit cycle instead of a fixed point, confirmed exactly (bit-for-
  bit repeating every other frame from roughly frame 30 onward, out to at least 400 frames tested).
  This is structural, not a one-off coincidence of this exact PCM: the same kind of short, stable
  cycle (not divergence, and not exact convergence) appears across a wide tone-amplitude sweep
  (3000-12000 -- expected, since Eq. 54's own bias-correction term is proven elsewhere in this
  codebase to cancel a constant level exactly, so uniform amplitude scaling only shifts the coarse
  Annex E gain index `b2`, never the shape-encoding coefficients that actually drive the cycle) and
  for a harmonic-rich (sawtooth-like) stimulus too. A related, separate finding from the same
  investigation: block 0's own `C_1,2` coefficient (5 bits at `L=18`) saturates at its quantizer's
  maximum index for this stimulus, since a pure tone concentrates essentially all real energy on
  exactly one harmonic out of 18 -- an input far outside the dynamic range Annex G's step sizes are
  presumably tuned for, consistent with the reconstructed dominant-harmonic amplitude settling
  noticeably *below* the true input level rather than merely oscillating around it. One block
  (harmonics 13-15, the smallest higher-order bit budget among the unvoiced blocks) doesn't even
  settle into a short exact cycle within 400 frames, though its magnitude stays bounded in the
  expected noise-floor range rather than diverging. Net assessment: a maximally degenerate,
  perfectly-stationary single-tone stimulus is a real stress test this closed-loop predictive coder
  was very likely never designed or tuned against (real speech is never this stationary or this
  spectrally concentrated), and the oscillation is the generic, expected outcome of a quantized DPCM
  loop with `rho` not close to 0 -- not a transcription bug. See
  `src/ambe/mod.rs`'s own `encode_frame_stays_bounded_for_a_stationary_tone_even_though_it_does_not_
  converge` test for the permanent regression guard this became (bounded amplitude + a checked exact
  period-2 cycle for the dominant harmonic, not full-frame convergence or full-frame periodicity --
  the latter was tried and found unreliable, since the slowest-converging block's own cycle length
  varies with how long the run is allowed to settle).
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

**Update, after the D-STAR investigation resolved (see §5): the right methodology for this mystery is
now known, and one prior "real next step" is already confirmed done.**

The D-STAR investigation found and fixed two real framing bugs (a genuine wire-level block interleave
matching D-STAR's own over-the-air format, and a spare-bit position error) using a **falsification
test** -- does a hypothesis Golay-decode real, noise-free chip output with *zero* corrected errors on
essentially every captured frame -- rather than the correlation-based or single-frame-epsilon
comparisons tried above, both of which turned out to be too weak a signal (Golay(23,12,7)'s
perfect-code property means a single frame's low epsilon proves nothing, and this document's own §3
already found near-chance bit-agreement and correlation scores for every hypothesis tried so far). The
same falsification methodology, systematically automated across *many* real captured frames rather
than hand-picked hypotheses on one frame, is the right next tool here too -- item 1 below reflects
this update; the old single-hand-picked-hypothesis approach in the "Update" paragraphs above is
superseded.

**A second, related lesson from D-STAR, PARTIALLY RETRACTED after checking AMBETools more closely --
see the correction immediately below.** The D-STAR investigation's own first attempt to test
`szechyjs/dsd`'s real interleave table *also* "failed" outright, exactly like every P25 interleave
hypothesis tried above -- not because the interleave hypothesis was wrong, but because a *second*,
independent bug (the spare-bit position) was corrupting the test at the same time. This general
lesson (two independent bugs can mask each other in a falsification test) still stands. But the
specific idea below it -- that `AMBETools`' own `IMBE_INTERLEAVE[144]` might be the *real DVSI chip's*
P25 wire format, the same way `dsd`'s `dW`/`dX` turned out to be D-STAR's -- is now believed **wrong**,
per the correction below. AMBETools' `IMBEFEC.cpp::decode()` (fetched and read directly) does confirm
three OTHER structural details already independently confirmed correct in this crate (see item 3
below): whitening bits `23..137` (all of `c1..c6`, none of `c0` or the final 7 raw bits), data-bits-
first-then-parity-bits-second within each field, and the same `23,23,23,23,15,15,15,7`-bit block
sizes. Those three remain useful, real confirmations. Only the interleave-table claim is retracted.

**Correction: `IMBE_INTERLEAVE[144]`/`CIMBEFEC` is NOT the real DVSI chip's wire format --
it belongs to a completely unrelated, from-scratch open-source IMBE implementation.** Checking
`AMBE2WAV.cpp` (fetched from `g4klx/AMBETools` directly) shows `CIMBEFEC` is only ever constructed
inside a `#if !defined(HAVE_USB3000_P25)` / `else if (m_mode == MODE_P25)` branch, printing
`"Using open source IMBE vocoder by Pavel Yazev"` and calling `imbe_vocoder::imbe_decode` -- i.e. this
whole code path is the **software fallback used when no real DVSI USB-3000 P25 chip is present**.
When a real chip *is* available (`HAVE_USB3000_P25` defined), execution instead falls straight through
to `CDV3000SerialController::process()`, which talks to the chip's own `CHANNEL` packets directly and
never calls `CIMBEFEC` or touches `IMBE_INTERLEAVE[144]` at all. So the earlier "8 hypotheses including
AMBETools' own interleave table, both bit-number conventions, all failed identically" result (§3 above)
was testing an interleave that was never expected to apply to the real chip's own CHANNEL bytes in the
first place -- its failure is not evidence against a real DVSI-specific interleave existing, it's just
evidence against *this particular, unrelated* table. Unlike D-STAR (where `dsd`'s interleave table came
from a real, independent, working GMSK *demodulator* for the actual over-the-air signal the chip itself
also has to be compatible with), **no publicly available, independently-sourced reference for DVSI's
own real P25 chip wire format is currently known** -- this is a harder, more genuinely open reverse-
engineering problem than D-STAR's turned out to be, and should be approached accordingly: a systematic,
automated search over a principled space of transformations (block-order permutations, byte order,
per-byte bit direction), not a search for "the one real published table" the way D-STAR's search could
be, since none is known to exist publicly for this specific case.

**Real next steps, in order:**
1. **Build a proper falsification-test harness for P25**, mirroring
   `examples/ambe_chip_validate_dstar.rs`'s own approach: capture *many* real chip-encoded frames at
   several tones (not one hand-picked frame), and score hypotheses by how many of those frames
   Golay/Hamming-decode with zero corrected errors on *all four* Golay blocks simultaneously (not just
   `c0`) -- the real, decisive signal the earlier single-frame/correlation attempts above couldn't
   provide. Since no known-real reference interleave table exists for P25 (unlike D-STAR's `dsd`
   tables -- see the correction above), search a principled space of transformations rather than a
   short hand-picked list: byte order (normal/reversed), per-byte bit direction (MSB/LSB-first), and
   -- the new axis this correction motivates -- **permutations of the 8 blocks' own order** within the
   144-bit frame (`8! = 40320`, tractable to brute-force in a release build, especially with a
   syndrome-table Golay/Hamming decoder instead of brute-force nearest-codeword search for speed).
   This crate's own `c0..c7` data-vs-parity bit order within each field is already confirmed matching
   AMBETools (item 3 below), so that axis likely doesn't need to vary in the search.
2. ~~Diagnose and fix the 12-frame oscillation on a stationary input, independent of the chip
   comparison -- a closed-loop predictive encoder should converge to a fixed point for a genuinely
   stationary input, not cycle.~~ **Done -- diagnosed, and it's real DPCM limit-cycle behavior, not a
   bug to fix.** See the updated bullet earlier in this section for the full mechanism (a `rho ~= 0.49`
   prediction-feedback loop amplifying a `G_hat_2` quantizer-boundary error into a stable period-2
   cycle) and `src/ambe/mod.rs`'s own `encode_frame_stays_bounded_for_a_stationary_tone_even_though_
   it_does_not_converge` regression test. The original "period of exactly 12 frames" framing turned
   out to be an artifact of checking too early in the settling transient, not a real, exact period --
   direct measurement out to 400 frames shows most harmonics settle into a stable period-2 cycle,
   while at least one low-bit-budget unvoiced block never settles into a short exact period at all
   (though it stays bounded) -- so "does the 144-bit frame repeat with a short period" isn't itself a
   reliable invariant; "do reconstructed amplitudes stay bounded and sane" is the real, checked one.
3. ~~Directly compare `modulation::pseudo_random_sequence`'s own LCG formula against the
   `173*p + 13849 mod 65536` construction found in AMBETools' `IMBEFEC.cpp`~~ **Done, confirmed
   matching exactly, and while checking it two more structural pieces were confirmed matching too** --
   not the source of this mystery, any of them:
   - `src/ambe/modulation.rs`'s own `pseudo_random_sequence` already uses the identical
     `pr[0] = 16*u0; pr[n] = (173*pr[n-1] + 13849) % 65536` construction, byte-for-byte the same
     recurrence AMBETools' `IMBEFEC.cpp` and D-STAR's own whitening both use.
   - `src/ambe/mod.rs`'s own `c0..c6` assembly (`fec::golay_encode`/`fec::hamming_encode`) reuses the
     same "data bits high, parity bits low, MSB-first" systematic convention confirmed for D-STAR --
     matching AMBETools' own `encode()`, which reads each field's data bits first (positions `0..12`
     for a Golay block, `0..11` for a Hamming block) and appends parity second.
   - `modulation_vectors`' own whitening scope (`m_hat_1..m_hat_3`, the three Golay blocks after `c0`,
     plus `m_hat_4..m_hat_6`, all three Hamming blocks -- `m_hat_0`/`m_hat_7` never whitened) is
     `3*23 + 3*15 = 114` bits, exactly matching AMBETools' own `decode()` (`bit[i+23] ^= prn[i]` for
     `i` in `0..114`, i.e. bits `23..137`) bit-for-bit.

   With PRN, Golay/Hamming data-vs-parity convention, and whitening scope all independently confirmed
   to already match a real working reference, the wire-level interleave (item 1) is now the most
   likely remaining axis, by elimination -- not just "one hypothesis among several" the way it looked
   before these three were checked.
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

## 7. P25 wire-format falsification-test harness: built, validated against known ground truth, and a genuine negative result against the real chip

Per section 3's "real next steps" item 1, a proper multi-frame falsification-test harness for the P25
wire-format mystery is now built: `examples/ambe_chip_validate_p25_wireformat.rs`. It captures many
real chip-encoded frames and scores candidate bit-order hypotheses by how many frames Golay-decode
with *zero* corrected errors on all four `c0..c3` blocks simultaneously -- the real, decisive signal
this document's own methodology section (end of §3) already established, rather than the
correlation/single-frame comparisons that misled the earlier attempts recorded in §3.

**What it tests, in three families:**

1. A cheap sliding-window scan (does a contiguous 23-bit or 15-bit window at some fixed offset land
   on a valid codeword across most captured frames?) across the 4 byte-order x per-byte-bit-direction
   combinations.
2. All `8! = 40320` permutations of which of the 8 fixed-size contiguous blocks (`c0..c3` at 23 bits,
   `c4..c6` at 15 bits, `c7` at 7 bits) occupies which position in the 144-bit frame, x the same 4
   byte/bit-direction combinations -- the harness this document's §3 asked for. Scoring correctly
   accounts for this codec's own confirmed content-dependent PRN whitening (§3 item 3): a candidate
   `c0` is checked for validity first (never whitened, so a valid `c0` directly gives real data via its
   own top 12 bits, no search needed), then `c1..c6` are dewhitened using this crate's own
   `ambe::modulation::modulation_vectors` (reused directly, not reimplemented) before being checked --
   both the dewhitened and raw (no-whitening-on-the-wire hypothesis) scores are tracked per hypothesis.
   Valid-codeword membership is checked via a precomputed 1-bit-per-codepoint bitmap (`golay_encode`
   over all 4096 data values, `hamming_encode` over all 2048), making each check O(1) rather than
   `golay_decode`'s own O(4096) brute-force search -- the full 40320-permutation x 4-byte/bit-hypothesis
   search runs in under 0.1 second in a release build against ~100-200 frames, nowhere near the "a few
   minutes" ceiling the task set.
3. A real, independently-sourced P25 Phase 1 IMBE OTA bit interleave: `szechyjs/dsd`'s
   `include/p25p1_const.h` (`iW`/`iX`/`iY`/`iZ`, ISC-licensed, fetched directly from GitHub -- numeric
   values transcribed for cross-reference only, no code copied, consistent with this project's
   established position on the mbelib tables in §4), applied via freshly-written Rust and cross-checked
   against `szechyjs/mbelib`'s real `imbe7200x4400.c` (`mbe_eccImbe7200x4400C0`/`Data`,
   `mbe_demodulateImbe7200x4400Data`) to confirm the resulting `imbe_fr[block][bitpos]` convention
   (bit weight `2^bitpos`), the data-first-MSB-first Golay/Hamming layout, and the PRN whitening
   formula/scope all match this crate's own `src/ambe/` exactly -- the same kind of primary-source
   cross-check that cracked D-STAR (§5), tried here because a real independent P25 demodulator has to
   be wire-compatible with the same over-the-air bitstream, and the AMBE3003's D-STAR CHAND bytes
   turned out to already be in real over-the-air order. Both dibit-bit-order conventions (which
   physical bit of each pair plays the demodulator's own "bit 1" role) are tried.

**The harness was validated against known ground truth before trusting any result against the chip**
(`--selftest`, no chip contact needed): it generates real frames from this crate's own from-spec
encoder (`ambe::encode_frame`, whose returned `c` is already fully modulated/whitened -- confirmed by
reading `ambe::mod::encode_code_vectors`) and packs them at the "natural" identity block order,
normal-byte, MSB-first convention. The permutation search correctly recovers this exact hypothesis
(`perm = [0,1,2,3,4,5,6,7]`, normal bytes, MSB-first) with **10/10 frames matching on all four
dewhitened Golay blocks** and, decisively, **10/10 on the full seven-block check** (four Golay +
three Hamming) -- while permutations that merely reorder the three same-sized Hamming blocks among
themselves still score 10/10 on the four-Golay-block metric (expected: that metric never touches
`c4..c6`) but collapse to near-zero on the seven-block metric, exactly the discriminating behavior
the two-tier scoring was designed to produce. This confirms the scoring pipeline (dewhitening via the
real `modulation_vectors` call, MSB-first bit extraction, membership-bitmap checks) has no
implementation bug that could hide a real positive.

**Live captures against the real chip**: two configurations, both via ordinary DVSI CONTROL/SPEECH UDP
packets (never touching the serial/USB layer, so the chip's UART-BREAK lockup hazard is not at risk):

- The task-confirmed P25 FEC RATEP rate-control-word (`0x0558 0x086B 0x1030 0x0000 0x0000 0x0190`):
  6 tones (100/150/200/300/400/600 Hz) x 2 amplitudes (2000/8000), continuous (not frame-relative)
  phase across frames for maximum bit diversity regardless of whether a tone's period evenly divides
  160 samples, 30 frames captured per combination after a 10-frame settling discard -- 360 frames
  captured, **125 unique** after dedup.
- RATET index 27 (this codec's own established default rate, same 144-bit frame, a different chip
  configuration path) as a secondary check on whether the two configs yield the same wire layout:
  3 tones x 1 amplitude, 90 frames captured, **31 unique**.

**Result: every hypothesis in every one of the three families scored at noise level for both
configurations.** The permutation search found **zero hypotheses** (out of 161280 checked per
configuration) reaching even the 2-frame noise-filter threshold on either the four-block or
seven-block metric. The dsd/mbelib OTA-interleave test scored **exactly 0 matching frames** for every
one of its 8 byte/bit/dibit-order combinations, on both configurations. The sliding-window scan found
no offset anywhere close to the ~100% match rate a true `c0` location should show (best Golay-window
hit was 3/125 frames, close to the 1/2048 per-frame chance rate x number of offsets tested, not a real
signal) -- consistent with, and independent confirmation of, the permutation search's own negative
result. RATEP(P25 FEC) and RATET(27) showed the same qualitative (negative) result, so this isn't
merely a property of one specific chip configuration.

**What this means, honestly**: given that the identical scoring pipeline decisively recovers the true
answer on synthetic ground truth, this is a real negative result, not a harness bug. It rules out,
for the real DVSI AMBE3003 chip's P25 `CHANNEL` bytes specifically:

- Any simple byte-order/per-byte-bit-direction combination with the 8 blocks left in their natural
  `c0..c7` concatenation order or any reordering of them as whole contiguous chunks (the full 8!
  search).
- The specific real P25 Phase 1 OTA bit-interleave `dsd`/`mbelib` use for actual RF demodulation, under
  either dibit-bit convention.

It does **not** rule out a genuine non-contiguous, bit-level interleave specific to DVSI's own chip
firmware that happens to differ from the standard's own over-the-air interleave -- which was always
the more likely outcome once the AMBETools `IMBE_INTERLEAVE[144]` table was ruled out as inapplicable
(see §3's correction) and no other independently-sourced reference for DVSI's *own* proprietary wire
format could be found. Unlike D-STAR, where the chip turned out to reuse the real over-the-air format
directly on its serial bytes, P25's chip apparently does not do the equivalent -- at least not via the
one real, independent interleave table checked here. Cracking DVSI's actual proprietary P25 bit
layout from here, if still wanted, would need either a genuine from-scratch structural search (a much
larger, non-contiguous permutation space than the one searched here, likely intractable to brute
force) or a different empirical approach entirely (e.g. correlating specific known-content chip
CONTROL/DATA channel bits against expected vocoder parameter values one at a time, rather than
guessing a global bit-order transformation) -- a genuine, open reverse-engineering problem with no
shortcut currently in hand, exactly as this document's own honest assessment in §3 anticipated.

**Reproducing this work**: `cargo run --release --example ambe_chip_validate_p25_wireformat --
192.168.10.189 2460 --save <path>` captures fresh frames from the chip and runs all three hypothesis
families (takes well under a minute total, dominated by the ~600 UDP round trips during capture, not
by the search itself). `--replay <path>` re-runs the analysis against a previously saved capture
without hitting the chip again. `--selftest` runs the ground-truth validation described above with no
network access at all.
