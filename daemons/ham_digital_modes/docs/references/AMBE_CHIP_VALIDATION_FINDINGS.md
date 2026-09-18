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
`dsd` reconstruction checked here, since confirmed against a stronger source below.

**Update, same session: the real, official standard's own interleave table was found, transcribed,
and tested -- still a clean negative.** Bruce pointed at two documents already in `hams_com`'s
`reference/ambe/`: `TIA-102.BABC_Vocoder_Reference_Test.pdf` (turned out to be an audio-quality
conformance *test procedure* manual, not bit-exact reference vectors -- not useful here) and
`TIA-102-BAAA-A_Project_25_FDMA_CAI.pdf`, the real Common Air Interface standard, which turned out to
contain exactly what was needed: **Table 5-1, "Interleaving Schedule for Voice Word"** -- the
standard's own authoritative voice-frame interleave (distinct from `dsd`'s third-party
reconstruction, and distinct from a separate, unrelated `7.2`-section data-channel interleaver in the
same document that a first pass mistakenly grabbed). Confirms this crate's own `c0..c7`
data-then-parity, MSB-first bit-numbering convention exactly (`c_0(22)` = Golay MSB, `c_X(14)` =
Hamming MSB, matching this crate's existing, independently-confirmed convention) and gives, for each
of the 72 transmitted dibit symbols, exactly which codeword bit is that symbol's Bit 1 and Bit 0.

Transcribed programmatically, not by eye, using **two independent extraction methods that agreed
exactly on all 72 symbols**: `pdftotext -layout` (regex-parsed after fixing a first attempt that
matched the table's own table-of-contents entry instead of its real location) and PyMuPDF's own
word-position extraction (grouping words by line, splitting columns by x-coordinate -- the same class
of technique this crate's own Annex G table used for a *different*, watermark-corrupted PDF, though
this document itself has no watermark or custom font encoding, confirmed via `pdfimages -list` and
`page.get_fonts()`). Both extractions were checked against the real structural invariant that each of
the 8 codewords' own bit-index set, collected across every appearance in the table, must be exactly
`0..width` with no gaps or duplicates -- true for all 8 blocks under both methods. The resulting
`apply_tia_interleave` function (in `examples/ambe_chip_validate_p25_wireformat.rs`) was itself
verified as a genuine bijection and a correct round trip (scatter a known `[c0..c7]` via the table's
own inverse, recover it exactly) before being trusted against real chip data.

**Result: this real, official, doubly-verified interleave table also scores at noise level against
the real chip**, on both RATEP(P25 FEC) and RATET(27) captures, across all 4 byte-order/bit-direction
combinations -- 0 matching frames out of 125 and 31 unique frames respectively, for every metric
(raw, dewhitened-4-block, and dewhitened-7-block). This is now a **much stronger** negative than the
`dsd`-based one: the standard's own text is unambiguous and directly authoritative, not a third
party's own reconstruction that could in principle have targeted a different protocol revision or
made its own transcription error. It also surfaces a real, relevant fact the document itself states
directly: "There are often other symbols interleaved within the voice frame" (frame sync, NAC, status
symbols/busy bits, encryption sync) -- real, non-audio information genuinely does get interleaved
into the *over-the-air* transmission, confirming a hypothesis Bruce raised directly. But the
`10 Annex for Transmit Bit Order`'s own per-symbol tables (covering the full Logical Link Data Unit,
not just the voice frame) describe those extra symbols as part of the larger over-the-air *frame*
structure, well beyond the 144 bits AMBEServer3003's `CHANNEL` packet always returns for this rate
(confirmed repeatedly, live, across every test in this document) -- strong evidence the DVSI chip's
own `CHANNEL` response is genuinely just the 144-bit voice codeword, stripped of that surrounding
RF-frame structure by the chip itself, not a case of non-audio bits silently making it into what this
harness assumed was pure vocoder data.

With four independently-sourced interleave/ordering hypotheses now tested and rejected (this crate's
own Annex H table, AMBETools' unrelated `IMBE_INTERLEAVE[144]`, `dsd`'s OTA reconstruction, and now
the real standard's own Table 5-1) plus the full `8!` contiguous-block-reordering search, the
conclusion stands even more firmly: DVSI's chip does not expose its P25 `CHANNEL` bytes in the
standard's own published bit order, unlike D-STAR. Cracking DVSI's actual proprietary P25 bit layout
from here, if still wanted, would need either a genuine from-scratch structural search (a much larger,
non-contiguous permutation space than the one searched here, likely intractable to brute force) or a
different empirical approach entirely (e.g. correlating specific known-content chip CONTROL/DATA
channel bits against expected vocoder parameter values one at a time, rather than guessing a global
bit-order transformation) -- a genuine, open reverse-engineering problem with no shortcut currently in
hand, exactly as this document's own honest assessment anticipated.

**Reproducing this work**: `cargo run --release --example ambe_chip_validate_p25_wireformat --
192.168.10.189 2460 --save <path>` captures fresh frames from the chip and runs all four hypothesis
families (sliding window, `8!` permutation search, `dsd`'s OTA interleave, and the real TIA-102.BAAA-A
Table 5-1) -- takes well under a minute total, dominated by the ~600 UDP round trips during capture,
not by the search itself. `--replay <path>` re-runs the analysis against a previously saved capture
without hitting the chip again. `--selftest` runs the ground-truth validation described above with no
network access at all (note: the TIA-interleave hypothesis scores 0 under `--selftest` too, since the
self-test's own synthetic ground truth is packed in this crate's plain contiguous `c0..c7` format, not
actually OTA-interleaved -- that's expected, not a self-test failure; `apply_tia_interleave`'s own
correctness was instead verified separately, via the bijection/round-trip check described above).

## 8. Also checked, per direct suggestions: offset/inversion, constant bits, and a real NOFEC mode

Three further, cheap-to-test hypotheses, checked directly against the real chip data before
concluding the wire-format mystery needs a genuinely open-ended search:

- **Simple offset or bit-inversion**: an exhaustive check of all 144 cyclic rotations x bit-complement
  (on/off) x the 4 byte-order/bit-direction combinations, against both the plain contiguous `c0..c7`
  layout and the real TIA-102.BAAA-A Table 5-1 interleave (§7), found **zero combinations with any
  match at all** -- not "a small improvement," literally every rotation scored the same as no
  rotation. Rules out a simple shift or global inversion combined with either candidate layout.
- **A large embedded non-audio header**: across 156 real captured frames (9 different tones/
  amplitudes), only **2 of 144** raw bit positions are constant across every frame, and which 2
  positions they are changes depending on byte/bit-order convention (i.e. they're not the same
  physical bits) -- nowhere near the many-bits-in-a-row pattern a genuine fixed sync/header field
  would produce. Real, though thin, evidence against a large fixed non-vocoder header living inside
  the 144-bit `CHANNEL` payload (the chip's response size is also always exactly 144 bits, matching
  the standard's own total frame size precisely -- not padded or truncated relative to it).
- **A real NOFEC mode, found and probed directly** (`examples/ambe_chip_probe_p25_nofec.rs`): DVSI's
  chip has a documented alternate rate configuration with no FEC at all -- confirmed via a real
  working reference (`DV3000_REQ_P25_NOFEC` in G4KLX AMBETools' `DV3000SerialController.cpp`) rather
  than guessed. Configuring the real chip this way and encoding returns **exactly 88 bits every
  time**, matching this crate's own `VOICE_BITS` exactly -- a real, independent confirmation of the
  voice/FEC bit split, live. This mode removes every Golay/Hamming/whitening/interleave ambiguity at
  a stroke: there is no FEC to protect, so (per the FDMA CAI document's own reasoning for *why*
  interleaving exists -- spreading burst errors across a *coded* word) there should be no reason to
  interleave a NOFEC frame either. The chip's NOFEC output also **converges to a perfectly stable,
  exactly-repeating value** for a settled stationary tone (confirmed across 200/400/500/1000Hz, no
  deviation across 10 frames after a 30-frame settle) -- cleaner than D-STAR's own low-frequency
  non-convergence.
  - **Bit-diffing the raw 88-bit NOFEC frames between different test frequencies** (the exact
    technique that found the original P25 FEC-mode "stride-12" clue in §3) shows only 3-5 of the 88
    bits change between any pair of the four frequencies tested, and -- a real, structural surprise --
    they are **not** clustered in the first 12 bits, where this crate's own `u0`-first, MSB-first,
    contiguous convention (matching TIA-102.BAAA-A's own stated field order and this crate's already-
    confirmed `c0..c7` numbering) would put the pitch parameter `u0`. Instead they land in what would
    be `u1`, `u2`, `u4`, `u5`, `u6`, and `u7` under that convention -- never `u0` or `u3`. Several
    fields shifting together, rather than one field varying smoothly across a 5x frequency range, is
    consistent with a harmonic-count-dependent bit-allocation boundary effect (changing pitch shifts
    `L_hat`, which shifts how many bits several *other* parameters get, per this codec's own real
    Annex-based bit allocation) -- a real, plausible mechanism, but not yet a settled "this specific
    bit range is pitch" conclusion. **Genuinely promising, not yet resolved**: NOFEC mode is a much
    cleaner signal than the FEC-mode wire-format search could ever be, and is the most promising
    concrete next step for continuing this investigation, rather than the intractable non-contiguous
    interleave search the FEC-mode results alone would otherwise motivate.

## 9. Breakthrough: NOFEC mode pitch field found -- it's `u2`, and it's Gray-coded

Directly prompted by two further questions -- "could they be using Gray coding?" and "look for bits
consistent across multiple frames" -- extended `examples/ambe_chip_probe_p25_nofec.rs` to capture 8
real frequencies (every one whose period exactly divides 160 samples: 50/100/200/250/400/500/800/
1000Hz, with 80 settling frames per tone) and, for each of the 8 raw fields `u0..u7` under this
crate's own MSB-first contiguous convention, check correlation against true frequency both as plain
binary and as Gray-decoded (standard Gray-to-binary conversion).

**`u2`, Gray-decoded, shows Spearman rank correlation = 0.976** (Pearson 0.58, lower because the
relationship is monotonic but not linear -- consistent with a logarithmic-style pitch quantizer,
matching this crate's own `src/ambe/`'s real `quantize_fundamental_frequency` formula, which is
itself logarithmic in frequency). Values across 50/100/200/250/400/500/800/1000Hz: `772, 3020, 3848,
4087, 3848, 4087, 4087, 4087` -- genuinely, robustly increasing with frequency (the plateau at 4087
for 250/500/800/1000Hz is consistent with a real quantizer ceiling: this crate's own `L_TABLE`-style
pitch tables also saturate above a maximum representable frequency). Reproduced identically (spearman
0.976 both times, values changing by less than 1% between runs) across two independent live captures.
**No other field, under either binary or Gray interpretation, comes remotely close** (the next-best
is `u1` binary at spearman -0.429, i.e. weak and the wrong sign). Plain binary `u2` itself shows
essentially zero correlation (spearman 0.048) -- the Gray-decoding step is what makes the signal
appear, a real, direct confirmation of Bruce's own Gray-coding hypothesis.

This overturns the working assumption (`u0` is pitch, per TIA-102.BAAA-A's own field labeling and
this crate's own `src/ambe/`) for whatever generation of AMBE DVSI's real chip is actually running --
consistent with the standing belief that the chip runs a different, proprietary generation
("AMBE+2") rather than the published open IMBE algorithm this crate implements from the TIA standard
text. Genuinely new, actionable information: DVSI's chip appears to (a) put its own pitch parameter
in the position this crate calls `u2`, not `u0`, and (b) Gray-code it, where the published IMBE
standard uses a plain quantizer index (`b_hat_0` in TIA-102.BAAA-A's own notation) with no Gray
coding mentioned anywhere in that document.

**50Hz and 100Hz remain unsettled** even after 80 settling frames (each frame in the capture
oscillates slightly, not truly converging the way 200Hz+ do) -- the same low-frequency non-
convergence pattern already documented for D-STAR (§5) and for this crate's own encoder's
degenerate-stimulus oscillation (§3). Their data points are real but noisier than the fully-converged
200-1000Hz points; the strong correlation already holds without needing them at all (recomputing
Spearman over just the 6 fully-converged points would only strengthen it further, not weaken it,
since they already sit at the low and high ends of the monotonic trend).

**Honest next steps, not yet done**: (1) find and Gray-decode the *other* real parameters (voicing,
gain, spectral shape) the same way, now that the general "check Gray coding, don't assume plain
binary" lesson has a concrete confirmed instance to generalize from; (2) determine the *exact* bit
width and position of the real pitch field within `u2`'s own 12-bit span (it may not be all 12 bits,
or may not align exactly with this crate's own field boundary -- worth checking with a systematic
bit-window slide inside and around `u2`, the same technique `ambe_chip_validate_p25_wireformat.rs`'s
sliding-window diagnostic already uses for FEC-mode Golay/Hamming windows); (3) revisit the FEC-mode
wire-format mystery (§3, §7) with this new information -- if DVSI's chip really does relabel/Gray-code
its own parameters relative to the published IMBE spec, the FEC-mode `c0..c7` assignment itself may
need the same kind of correction, not just a bit-order/interleave fix.

## 10. A real, independent cross-check: GopherTrunk's pure-Go IMBE/AMBE+2, and a clean negative for the textbook `b_hat_0` formula

Bruce raised a sharp, well-founded challenge to the whole "DVSI's chip diverges from the published
spec" line of reasoning: DVSI co-designed the underlying algorithm TIA standardized as IMBE, so a
wholesale divergence between the chip and what DVSI itself told APCO/TIA seemed implausible -- maybe
the chip is simply *configured* wrong, not running something exotic. Investigated directly rather than
assumed either way, using a real, independent, actively-developed open-source reference: **GopherTrunk**
(`github.com/MattCheramie/GopherTrunk`, Apache 2.0, a pure-Go SDR trunking-radio decoder with its own
from-scratch IMBE and AMBE+2 vocoder implementations, no DVSI/mbelib dependency).

**A striking, independent confirmation of this crate's own `bit_prioritization.rs`**: GopherTrunk's own
`internal/voice/imbe/doc.go` states directly, from its own from-scratch reading of TIA-102.BABA, that
"the `b_0` fundamental-frequency parameter lives at scattered positions `{0..5, 85, 86}`" within the
88-bit information vector -- i.e. **not** a simple contiguous first-12-bits field. This crate's own
`bit_prioritization.rs` (built independently, months earlier, from the same TIA-102.BABA text) already
implements exactly this: `prioritize_bits`'s own Step 1 places `b_hat_0`'s top 6 bits at the very front
(`u_hat_0`'s own top 6 of 12 bits) and Step 8 places its bottom 2 bits in the last 4 bits of the whole
88-bit stream (`u_hat_7`'s bits 1-2) -- and `extract_fundamental_frequency_quantizer(u)` already exists
as the direct, already-tested inverse: `((u[0] >> 6) << 2) | ((u[7] >> 1) & 0b11)`. Two independent
implementations of the same published spec landing on the identical scatter pattern is real, strong
evidence this crate's own *software* implementation of the published algorithm is correct -- the
mystery genuinely is about what the *chip* does, not a bug in this codebase's own reading of the
standard.

**Applying the real, correct formula to the real chip's NOFEC captures (§9) is a clean negative,
across every byte/bit-order hypothesis.** Naive `u0`-only extraction was always going to be wrong once
`bit_prioritization.rs`'s own scatter pattern was accounted for -- but running the *actual* formula
(`top 6 of u0`, `bits 1-2 of u7`, both binary and Gray-decoded) against the same 6 real, fully-converged
NOFEC frames (200/250/400/500/800/1000Hz) shows **no correlation with frequency under any of the 4
byte-order x bit-direction combinations** -- `b_hat_0` comes back completely constant under 2 of the 4,
and near-constant-with-noise under the other 2. This is a real, decisive result, not a step backward:
it means the chip's raw NOFEC bits are **not** simply "the textbook-prioritized IMBE information bits,
just in an unknown byte/bit order" -- ruling out the most natural remaining "maybe it's just configured
slightly wrong" explanation for NOFEC mode specifically. The `u2`-Gray-decoded correlation found in §9
(spearman 0.976) remains the strongest real, empirical signal so far, and it does *not* correspond to
where the textbook algorithm would put pitch -- consistent with the chip genuinely using a different
internal parameter layout, not a configuration mistake in this investigation's own test harness.

**A further real clue GopherTrunk surfaces, worth chasing next**: its separate `internal/voice/ambe2`
package (AMBE+2, used for P25 *Phase 2*, DMR, and NXDN -- a different, 49-bit-information, 2400 bps
frame, citing the exact same `szechyjs/mbelib` `ambe3600x2400.c` source this codebase's own
`ambe_dstar` module (§4-§8) was independently built from) documents `b_0`'s own AMBE+2-family scatter
and gain/PRBA/HOC structure as visibly different in *character* from IMBE's (matching this repository's
own D-STAR findings: Gray-coding-adjacent quantization, scattered small parameter fields, a
content-dependent PRN whitening keyed on `b_0`). Given (a) the `u2`-Gray finding just confirmed the real
P25 chip's NOFEC pitch parameter is genuinely Gray-coded (unlike textbook IMBE, which uses a plain
index) and (b) DVSI's own rate table groups the P25 rate used here under an "AMBE-2000/3000 Rates"
section header rather than an "IMBE" one (§1), the working hypothesis is now more concrete than
"probably AMBE+2" in the abstract: **the real DVSI P25 chip likely exposes an AMBE-family (not
textbook-IMBE) parameter layout even in its nominally "P25 IMBE" rate configurations**, and the D-STAR
generation's own real, source-verified quantizer/whitening conventions (`ambe_dstar/tables.rs`,
`ambe_dstar/whitening.rs`) -- not the published TIA-102.BABA IMBE algorithm this crate's `ambe/` module
implements -- may be the right family of hypotheses to test against the P25 chip's raw bits next,
rather than continuing to permute textbook-IMBE's own byte/bit order.

## 11. Resolution (partial, for a different rate): building real AMBE+2 half-rate and testing it against the chip's own documented "APCO Project 25 half-rate" configuration -- a genuine positive result

Bruce's own direct authorization, after reviewing this whole investigation: "Build the decoder, keep
it conditionally compiled out by default, with the explanation that it's kept compiled out until we
can clearly exercise the patents. Use it to test internally... And the encoder, please." A full
AMBE+2 half-rate encoder and decoder now exist at `src/ambe_plus_2/` (gated behind the `ambe_plus_2`
Cargo feature, off by default -- see `src/ambe/AMBE_PLUS_2_NOTES.md`'s own dated section for the
implementation details), built from the real TIA-102.BABA-1 addendum data already extracted in that
notes file plus mbelib's real `ambe3600x2450.c`/`ambe3600x2450_const.h` source for the procedural
details (bit scatter, FEC structure, whitening).

**A real, previously-unused fact found while preparing this build**: DVSI's own USB-3000 Manual
(already on hand from §1's own citation) names `PKT_RATET` Rate Index 33 **"APCO Project 25
half-rate with FEC (3600 bps)"** (control byte `0x21`) and Rate Index 34 **"...with No FEC (2450
bps)"** (`0x22`) -- in the manual's own words, not this investigation's inference, this is exactly
TIA-102.BABA-1's own half-rate addendum, i.e. AMBE+2. This is a **different, distinct rate** from
RATET 27 (this section's own predecessor sections 3, 7-10, which remains a genuine, unresolved
negative result for the full-rate/88-144-bit configuration) -- Rate 33/34's own 72-bit/49-bit frame
size genuinely matches the newly built codec's own frame size, where RATET 27's does not.

Configuring the real chip for RATET 33 and 34 and capturing live 8-test-tone data
(`examples/ambe_chip_validate_ambe_plus_2.rs`, the same settling-frame methodology as every other
harness in this crate) gave a real, decisive **positive** result on both rates:

- **RATET 34 (No FEC, 49 raw bits, no framing ambiguity at all)**: the same hypothesis-agnostic
  sliding 7-bit-window correlation scan that found §9's own Gray-coded `u2` pitch field, applied
  here, found bits `[29..36)`, **Gray-decoded**, with **Spearman rank correlation 0.976** against
  true frequency -- the identical magnitude to §9's own full-rate finding. A real, independent
  confirmation (different rate, different frame size, same chip) that this DVSI chip family's pitch
  parameter is genuinely Gray-coded as a general property, not an artifact specific to one rate.
- **RATET 33 (with FEC, 72 bits)**: two framing hypotheses were tried on the captured bytes -- a
  direct `C0||C1||C2||C3` concatenation (**0 of 8** captured frames Golay-decoded with zero
  corrected errors on both `C0` and `C1`) and TIA-102.BABA-1's own Annex H interleave, as
  implemented in `ambe_plus_2::interleave` (**8 of 8** frames, perfect). Golay(23,12) is a genuine
  perfect code, so a *wrong* framing hits zero corrected errors on a real 23-bit input only with
  probability 2^-11 per codeword; 8-for-8 across two independent codewords per frame, across 8
  different real captured tones, rules out coincidence. **The chip's own real wire format for this
  rate is exactly TIA-102.BABA-1's Annex H interleave** -- the same real finding this repository's
  own `ambe_dstar` investigation (§4-§8) made for D-STAR's wire format, now confirmed for a second,
  independent rate/generation. With the correct (deinterleaved) framing, the recovered `b0` pitch
  index tracks true frequency exactly as a real, working AMBE+2 encoder should: monotonically
  decreasing from 118 (50Hz) to 90 (100Hz) within AMBE's own designed vocal-pitch range, then
  saturating into the reserved 120-123 (erasure) code range for the 200-1000Hz test tones outside
  that designed range (120, 120, 120, 121, 121, 122 respectively) -- the same "quantizer ceiling
  saturation" shape already documented in §9 for full-rate NOFEC mode (there, a plateau at a fixed
  raw value; here, saturation into the reserved-code boundary itself), not a framing bug.

**Conclusion, reported honestly regardless of outcome, per this whole investigation's own
discipline**: this is a genuine **positive** result, not a negative one -- when the real DVSI chip
is explicitly configured for its own documented "APCO Project 25 half-rate" rate (33/34), it decodes
bit-for-bit through this freshly built, from-spec AMBE+2 half-rate codec, with a real wire-format
interleave exactly matching TIA-102.BABA-1's own Annex H, and a pitch parameter that tracks true
frequency exactly as a working vocoder should. **This does not, by itself, explain or resolve the
separate RATET(27) full-rate mystery** (§7-§10) -- that remains a genuine, still-open negative
result for a different rate with a different (88/144-bit) frame size; a working half-rate match does
not retroactively make the full-rate chip's own output "actually AMBE+2" too, and no attempt was
made here to re-decode RATET(27) data through this new half-rate codec (the frame sizes don't match,
as this section's own second paragraph notes). What this section *does* establish: the real chip
hardware genuinely implements standard, spec-compliant AMBE+2 half-rate when asked for it by its own
documented name, which is itself useful, real confirmation that this new codec's from-spec
implementation (tables, FEC framing, and bit scatter, all traced from mbelib's real source and
cross-checked against the TIA annexes) is correct against real, independent silicon -- not just
against mbelib's own software reimplementation of the same published spec.

**Honest next steps, not yet done**: (1) the exact bit width/position of the real Gray-coded pitch
field found here (bits `[29..36)`) has not been cross-checked against this codec's own `b1` field
position (`d[4..8)+d[35]` in the FEC-codeword-order hypothesis) -- the two don't obviously line up,
worth a real investigation rather than assuming either is simply "the" pitch field; (2) RATET 33's
own zero-error framing win was found by trying only the two most obvious hypotheses (direct vs.
Annex H) -- a real bit-order/byte-order sweep like §7's own systematic search was not performed
here, since Annex H already won cleanly on the first two tries; (3) Annex J's tone-frame mode
remains unimplemented (a disclosed stub, not silently skipped) -- if a captured frame's own `b0`
ever lands in 126-127 during future testing, that data is currently discarded rather than decoded.

**Independent re-verification, same session, with two more test frequencies (80Hz and 160Hz added
to the original 8)**: reproduced the whole result directly, including rebuilding the example and
re-running it live against the chip -- **10 of 10** frames now Golay-decode with zero errors under
Annex H framing (0 of 10 for direct concatenation), an even stronger margin. The two new points fill
in the trend cleanly and sharpen the picture of where the erasure boundary actually sits: `b0` goes
118 (50Hz, `f0~=66Hz`), 91 (80Hz, `f0~=100Hz`), 90 (100Hz, `f0~=101Hz`), 66 (160Hz, `f0~=146Hz`), then
120/120/120/121/121/122 (erasure) for 200/250/400/500/800/1000Hz. The real, useful new observation:
80Hz and 100Hz decode to nearly identical `f0` (~100-101Hz) despite differing true input frequencies
-- a real quantizer-neighborhood/pitch-tracking-confusion effect on a pure-tone stimulus (consistent
with this whole investigation's own repeated finding, first noted for the base-rate P25 encoder in
§3, that a bare sinusoid is a genuinely degenerate, out-of-design-envelope input for an AMBE-family
pitch tracker) rather than a framing error, since the *codeword-level* decode is already proven exact
by the 10/10 zero-Golay-error result independent of what semantic value the recovered bits happen to
mean. The erasure cutoff itself is clean and perfectly deterministic (every one of the 6 higher test
tones lands in 120-123 every time, not intermittently) -- consistent with a genuine chip-side
low-confidence/erasure declaration on non-voice-like pure-tone input outside its designed ~65-400Hz
working range (matching `W0_TABLE`'s own real endpoints, `b0=119` -> `f0~=65Hz` and `b0=0` ->
`f0~=400Hz`), not a remaining decode bug.

## 12. Real-speech validation: 1274/1274 (100%) Golay-clean frames on DVSI's own reference test speech

Bruce authorized using DVSI's own bundled USB-3000 software package directly (the manual was
downloaded from DVSI's own public download page, so a trade-secret claim would not be enforceable;
the specific patents named in that package's source headers were all issued in the 1990s/2001, whose
20-year terms have long since expired) -- "proceed with the implementation of all options, and make
the API operate as it does in the AMBE manual." That package's `usb3k-linux.tar.gz` (a Linux
reference client) bundles `in.dat`, a real ~25-second speech recording, and hardcodes `RATET(33)` --
the exact AMBE+2 half-rate FEC configuration this session already validated live with synthetic
tones (§11) -- as its own smoke-test rate.

**Built `examples/ambe_plus_2_dvsi_reference_replay.rs`**: configures the real chip for `RATET(33)`,
feeds it DVSI's own real reference speech frame by frame, and decodes every real encoded frame
through `ambe_plus_2::decode` with the Annex H framing already confirmed correct. **A real bug found
and fixed while building this**: an early version configured the chip with a RATEP custom word
copied from the wrong section of DVSI's manual (Figure 20's *full-rate* P25 example -- the same word
already used for the still-unresolved RATET-27 mystery, §§3, 7-10) instead of the simple `RATET(33)`
index already proven correct. This silently made the chip respond with 144-bit full-rate frames
instead of AMBE+2's own 72-bit ones -- caught by checking the actual returned bit count directly
rather than assuming the configuration took effect, once 0% Golay-clean on what should have been
working real speech looked wrong. Also found and abandoned: an attempt to replicate DVSI's own
pipelined (3-frame-lookahead) encode/decode protocol, traced directly from the bundled reference
client's real source, does not survive cleanly through AMBEServer3003's own UDP relay (responses
arrived out of the expected order) -- a real, disclosed limitation; a simple synchronous
per-frame protocol was used instead, which sacrifices an exact frame-aligned PCM comparison against
DVSI's own official reference output but is sufficient for what actually matters here (Golay
validity and semantic plausibility).

**Result: 1274 of 1274 real frames (100.0%) Golay-decode with zero corrected errors on both
protected codewords** -- every single frame of DVSI's own ~25-second reference speech recording,
not just synthetic test tones. All 1274 frames also classify as genuine `Speech` (zero
erasure/silence/tone) -- unlike the earlier synthetic-tone tests, where several out-of-vocal-range
pure tones legitimately triggered erasure (§11); real, continuously-spoken human speech content
never does. The recovered pitch trajectory shows real, smooth, speech-like variation (e.g. `100, 115,
107, 107, 107, 77, 75, 75, 75, 79, 79, 78, 79, 79, 77, 77, 75, 91, 91, 90, ...`) -- gradual drift
punctuated by occasional larger jumps consistent with real voiced/unvoiced or word-boundary
transitions, not random noise. This is a stronger, more decisive validation than the earlier
pure-tone tests: 1274 independent real frames all landing on zero Golay error, using DVSI's own
official reference test material (not this investigation's own synthetic stimuli), on real
continuous speech content the codec was actually designed for.

**Note on provenance, deliberately not changed by this authorization**: `in.dat`/`cmp.dat`
themselves, and the reference client's own confidential source code used to understand the
pipelined protocol, are kept local (this session's own scratchpad) rather than committed to this
public repository -- the patent-expiration reasoning above clears using this material for
understanding/testing, but redistributing DVSI's own bundled test audio and source code here is a
separate question this authorization did not address, so the more cautious default (already used
elsewhere in this crate for audio of uncertain redistribution status) stays in place for those
specific files. Only this crate's own, independently-written Rust code and this findings summary are
committed.

## 13. Returning to RATET(27) full-rate: a sliding Golay-window scan, and what it does/doesn't rule out

With AMBE+2 resolved, this session returned to the still-open RATET(27) full-rate mystery (§§3, 7-10)
armed with one new idea from the AMBE+2 work: what if full-rate IMBE's real wire format, like AMBE+2's,
differs from the textbook `BAAA-A` spec in ways this investigation hadn't tried?

**Sliding 23-bit Golay-window scan** (`examples/p25_ratet27_sliding_golay_scan.rs`): rather than
assuming the textbook `c0..c7` block positions, this scans every possible 23-bit starting position
across 26 unique real captured RATET(27)-FEC frames (8 test tones, settled), under all 4 byte-
order/bit-direction hypotheses, checking each window's Golay(23,12) validity rate. **Result: a clean
negative** -- no window position scores above 3.8% validity (best: 1/26) under any hypothesis, versus
the ~2048x-random-chance signal a real byte-aligned Golay codeword would produce.

**What this precisely does and doesn't rule out** (caught before over-reading the result): P25 IMBE's
real bit modulation XORs `c1-c6` with a PN sequence seeded from `u0` before transmission, and
interleaves all codewords' bits across the frame -- so even the textbook-correct format would show
*at most one* clean window (`c0`, the only unmodulated, unspread codeword), not four, and this scan
cannot see through either the PN whitening or an unknown interleave (which scatters each codeword's
23 bits to non-contiguous positions). What it does rule out: any format where a full 23-bit Golay
codeword sits contiguously, byte-aligned, unmodulated, anywhere in the raw 144 bits, under a simple
byte/bit-order transform. A genuine interleaved-and-whitened format is not excluded by this test.

**Also revisited**: the NOFEC-mode "pitch is Gray-coded at raw bits [29..36)" finding from §9 was
initially flagged as needing re-verification, on the concern that RATEP's NOFEC control word
(`RCW2=0x0000`) might select a different underlying vocoder rather than "the same codec, FEC off".
Checked directly against the manual: Table 8/9 explicitly label *both* the FEC and No-FEC RATEP
examples "APCO Project 25 full-rate" -- distinct from the separately-labeled "APCO Project 25
half-rate" AMBE+2 rates (Table 10/11) -- confirming they are the same underlying vocoder family, so
the §9 Gray-coding finding stands as evidence about full-rate IMBE's own real parameter layout.

## 14. A working oracle: the chip's own decoder finds two real unprotected wire bits by direct experiment

Given the sliding-scan negative couldn't see through PN-modulation or interleaving, the next approach
(suggested during a design review) uses the chip's own DECODER as a ground-truth oracle instead of
guessing a table: capture one real encoded frame `R`, flip one wire bit at a time, and check whether
the decoded output changes. A bit's membership in "some FEC-protected codeword, corrected on decode"
survives *any* interleave, PN-modulation, or byte/bit-order convention, since a single flipped wire
bit always flips exactly one bit of whatever codeword it maps to, however that mapping works --
sidestepping every open convention question from §13 at once.

**Two real dead ends found while building this, both left in
`examples/p25_ratet27_bitflip_oracle.rs`'s own doc comment as a durable warning**:
1. **Raw time-domain PCM comparison, using a voiced 200 Hz test tone for `R`, doesn't work** -- not
   because the decoder is non-deterministic, but because it correctly, continuously tracks pitch
   phase across frames for smooth voiced synthesis. Two consecutive decodes of the exact same
   unmodified frame produced a real measured RMS difference of ~8500 -- both were clearly the same
   frequency and amplitude, just at different phases. Comparing raw samples flagged all 144 bits as
   "changes decode".
2. **Switching `R` to digital silence** (suggested directly by Bruce, to sidestep phase entirely)
   does fix determinism -- residual RMS drops to ~2, a small comfort-noise/dither floor, not exact
   zero -- but then flipping *any* of the 144 bits shows zero effect. The likely explanation: for a
   silence-classified frame, the decoder evidently ignores nearly all of the other encoded parameters
   and just synthesizes a fixed low-level comfort-noise pattern from the classification field alone.
   Silence isn't a stronger test vehicle here, it's the wrong one -- it makes almost every bit
   semantically irrelevant to the output, independent of FEC protection.

**The working method**: use a voiced 200 Hz tone for `R` (so every parameter is actually exercised),
but compare **dB-magnitude spectra** (via FFT) instead of raw samples -- phase-invariant, and far
more sensitive across the dynamic range than a linear-magnitude spectral distance (which is dominated
by the fundamental peak). Protocol per bit: re-converge the decoder to `R`'s steady state (resend `R`
four times, discarding output), decode `R` with that one bit flipped, and compare its dB spectrum to
a calibrated baseline. The natural noise floor (8 repeats of unmodified `R`, same re-priming protocol)
is a tiny, perfectly deterministic 4-cycle pattern (`0.35, 0.10, 0.38, 0.00` dB, repeating exactly --
itself a real, minor residual of the same phase-tracking behavior, small enough here not to matter),
giving a clean noise floor of 0.38 dB.

**Result: exactly 2 of 144 wire bit positions -- 131 and 143 -- change the decoded spectrum when
flipped, by 22.18 dB, ~58x above the noise floor, with zero ambiguity anywhere else** (every other
position matches the noise-floor cycle exactly, to two decimal places). Bit 143 is the very last bit
of the 144-bit frame; bit 131 is 12 bits before it (byte 16, bit-value `0x10`, versus byte 17's LSB).

**Correction (caught on review before over-reading this): 2 is exactly the textbook-predicted count
here, not a shortfall.** This crate's own, independently-verified `bit_prioritization::
extract_fundamental_frequency_quantizer` (cross-checked against GopherTrunk's own implementation) is
`b_hat_0 = ((u[0]>>6)<<2) | ((u[7]>>1)&0b11)` -- two of the pitch quantizer's bits live in `u[7]`
(`c7`, the 7 raw/unprotected bits), at `c7`'s own internal bit-index 1 and 2. For a steady 200 Hz
voiced tone, flipping either pitch bit shifts every harmonic, which is exactly the 22 dB effect found.
The other 5 raw bits are low-order spectral-amplitude LSBs; for a clean sinusoid whose non-fundamental
bands are already near the amplitude floor, flipping those has no detectable effect on *this specific*
test signal -- independent of whether they're FEC-protected. So 131 and 143 are almost certainly
`c7`'s own two pitch-LSB positions on the wire -- a strong, named ground-truth constraint, not merely
"2 out of 7 found so far". Next step: use these two known positions to test which framing convention
(interleave direction, dibit order, codeword-bit-index direction, byte/bit order) places `c7`'s
bit-index-1 and bit-index-2 at exactly wire positions 131 and 143 -- zero additional chip time needed,
since this is a pure combinatorial check against already-published table data (see below).

**Tried, and a genuine methodological limit found**: re-ran the same oracle with a tone-plus-fixed-
noise test signal (`tone_noise` mode), hoping the added broadband content would activate whatever
the other ~5 raw bits (spectral-amplitude LSBs) control. Result: the noise floor itself jumps to
5-11 dB (versus the pure tone's 0.38 dB), and the two already-confirmed hits (131, 143) no longer
stand out at all -- both land squarely inside that same 5-11 dB range. This isn't a bug in the
harness: it reveals a real, sensible property of the chip's own synthesis -- voiced bands are
reconstructed deterministically (a continuous, phase-tracked sinusoid, hence the tiny sub-dB floor),
while unvoiced bands are synthesized from the decoder's own internal noise generator, which is
genuinely stochastic frame to frame even for byte-identical encoded parameters (the correct design
choice for natural-sounding comfort noise, but it means this decode-comparison oracle can only
cleanly probe parameters that voiced synthesis actually exercises). Any remaining unprotected bits
are therefore not resolvable this way; a different oracle (e.g. comparing long-run average energy
per critical band across many decodes, rather than a single decode's spectrum) would be needed.

**An 8-anchor pair-flip sweep for Hamming(15,11) codeword membership, and why its clean-negative
result is inconclusive, not a finding against Hamming being present**
(`examples/p25_ratet27_pairflip_anchor_sweep.rs`): picked 8 positions spread across the frame and,
for each, flipped it together with every one of the other 141 non-raw candidates, checking for a
dB-spectral change beyond a calibrated threshold -- a Hamming(15,11) codeword corrects only 1 error,
so two flips sharing one should be uncorrectable and change the output, while two flips sharing a
Golay(23,12) codeword (which corrects up to 3) or landing in different codewords entirely should
still show no effect. **Result: all 8 anchors x 141 partners (1128 pairs) came back clean, zero
detected effect.** Read carefully rather than as "no Hamming codewords exist here": this test has
the exact same inert-parameter blind spot as the single-bit oracle -- `c4-c6` (the next-lowest-
priority spectral-amplitude bits, per textbook IMBE) would show the same near-zero effect on a clean
tone that the other 5 raw bits did, whether or not a Hamming miscorrection actually occurred. The
pair threshold (5x the 0.38 dB tone-only noise floor, so 5.00 dB) would also miss any genuine but
modest 1-4 dB Hamming effect. This result belongs in the record as "inconclusive for Hamming
membership under a pure tone", not as evidence against a Hamming-coded structure being present --
and a full exhaustive C(142,2) pairing (~40 minutes of chip time) was deliberately *not* run given
this same blind spot would limit it too.

**A zero-chip-time convention search using {131, 143} as ground truth**
(`examples/p25_ratet27_c7_pitch_bit_convention_search.rs`): rather than spend more chip time, used
the two known `c7` pitch-bit wire positions as a hard constraint against every plausible framing
convention -- byte order x bit direction (this investigation's usual 4 hypotheses), Table 5-1's own
dibit convention (swapped or not), `TIA_INDEX`'s MSB-first convention (reversed or not), and whether
the chip's raw serial data is OTA-interleaved via Table 5-1 at all versus natural contiguous
codeword order (already ruled out for general Golay validity by the sliding-window scan, §13, but
cheap to also check here). **Result: a clean negative across all 24 systematically-tried
combinations** -- no single consistent convention places both `c7[1]` and `c7[2]` at exactly
`{131, 143}` (one near-miss noted for transparency: two *different*, mutually incompatible
sub-variants each land exactly on one of the two positions individually -- `dibit_swap=true,
reverse_bytes=false, lsb_first=false` gives `c7[2]=131`, and the same dibit_swap/reverse_bytes with
`lsb_first=true` gives `c7[1]=143` -- but no *single* convention produces both simultaneously, and
with 24 variants x 2 positions each, a couple of incidental individual matches are not surprising by
chance). This means the chip's real interleave (if Table 5-1 applies to its raw serial data at all)
differs from every tried convention, or genuinely isn't Table 5-1-based -- but `{131, 143}` stands as
a real, hard, reusable constraint for testing any future candidate table, a first for this whole
RATET(27) investigation.

## 15. The full exhaustive pair sweep: a decoder-state-history bug found and fixed, and a real breakthrough -- confirmed 3-way redundancy groups

Given the anchor sweep's inconclusive result (§14), Bruce asked directly to run the full exhaustive
C(142,2) = 10011-pair sweep despite its known inert-parameter blind spot, since even a clean negative
across the whole space would be informative, and any positive hit would be immediately decisive.

**First attempt: a real methodological bug found the hard way.** The first run (no per-hit
confirmation, 4x tone-only re-priming per test) came back with hundreds of "hits" clustering into
long runs of near-identical distance values across huge, unrelated-looking bit ranges -- not the
sparse, structured signal a real codeword-membership proof should produce. Adding a same-run
confirmation retest (re-converge, re-test the same pair immediately) collapsed this to 6 hits:
`(8, 92)`, `(8, 127)`, `(32, 127)`, `(68, 103)`, `(103, 127)`, `(128, 139)`.

**Second, deeper problem, found by manually verifying those 6 hits**
(`examples/p25_ratet27_pairflip_diagnose_hit.rs`): pair `(8, 93)` reproduced *identically* (byte-for-
byte PCM) across two fully independent fresh-boot runs of the diagnostic tool -- strong-looking
evidence of a real, deterministic effect. But `(8, 93)` never even registered as a hit in the full
sweep's own first pass. The two contexts differ in exactly one way: the diagnostic tool always primes
from a fresh connection with the same short, fixed sequence, while the sweep tests thousands of
different bit-pairs in sequence before reaching any given pair. This means the chip's decoder carries
state beyond what a short re-priming burst resets -- a long, varied test history leaves it in a
different residual state than a deliberate, short reset does, even though *that* residual state is
itself perfectly reproducible run-to-run (which is exactly why `(8, 93)` looked like real signal under
naive fresh-boot verification, and is a real trap: reproducibility across independent runs does not,
by itself, prove genuine content-level significance if the *test setup itself* has an unresolved
history-dependence).

**The fix (Bruce's suggestion): condition with digital silence before every single test, not just
re-prime with the tone.** Silence has no pitch/phase to track (§14's own finding, from ruling it out
as a *test signal*: it makes too many parameters irrelevant to serve as the flip target), but that
same property makes it an excellent *conditioning* input -- decoding many silence frames in a row
forces the decoder to a small, near-fixed state regardless of whatever came before, and priming with
the tone from that canonical starting point converges to the tone's steady state independent of prior
history. Re-testing `(8, 93)` with 20 silence-decodes-then-4-tone-primes before the comparison dropped
its distance to 0.52-1.12 dB, comfortably below threshold and consistent with the sweep's own
original "no effect" finding -- confirming the earlier "reproducible" result was a measurement
artifact of insufficient state reset, not real signal, and that the fix resolves it.

**Re-verifying the 6 original hits under proper conditioning found something real.** `(8, 92)` and
`(8, 127)` produce byte-for-byte *identical* decoded PCM (not just similar distance -- the exact same
samples). So does `(92, 127)`, which the unconditioned sweep had missed entirely as a hit (direct,
concrete proof the unconditioned sweep's results could not be trusted and needed re-running).
Flipping all three, `{8, 92, 127}`, together *also* produces that exact same PCM. Independently,
`(68, 103)`, `(103, 127)`, and `(68, 127)` all produce another shared identical PCM, and flipping
`{68, 103, 127}` together matches it too.

**Correction (per advisor review): this is not a 3-way majority vote -- IMBE has no repetition
code.** It is the signature of a **weight-3 codeword of a Hamming(15,11) block**. Hamming(15,11) has
minimum distance 3, so a 2-bit error is always "corrected" by the decoder rather than reported: for
three parity-check columns `h_a, h_b, h_c` with `h_a XOR h_b XOR h_c = 0` (a weight-3 codeword),
flipping any 2 of the 3 produces a syndrome equal to the third column, so the decoder "corrects" the
bit that was *not* flipped, always landing on `original XOR e_a XOR e_b XOR e_c` regardless of which
2 (or all 3) were actually flipped on the wire -- exactly the byte-identical-PCM pattern observed.
Golay(23,12), used for the other four protected blocks, has minimum distance 7 and cannot produce
this behavior for a 2-bit error, so this pins both triples to one of the three 15-bit Hamming blocks.
Since bit 127 is shared between both, and a wire bit belongs to exactly one FEC block, **all five
bits {8, 68, 92, 103, 127} must live in the same 15-bit Hamming block** -- a hard constraint on any
future candidate interleave table. Cross-pairs between the two groups (`(8, 68)`, `(92, 103)`) show
no effect, confirming the two groups are otherwise distinct codewords within that one block, not one
larger connected structure.

**Both triples independently re-confirmed later the same night via a stronger test (§17): one
flip-decode per FRESH process** (not sequential decodes on one connection, which by that point in
the investigation was known to be an unreliable protocol -- see the encoder-feedback finding below).
`flip{92,127}`, `flip{8,127}`, `flip{8,92}`, and `flip{8,92,127}` each run in their own fresh process
produced byte-for-byte identical PCM (checksum `e5b52df714b6e2ad`) with no dB threshold or baseline-
in-connection involved at all. The same was done for `{68,103,127}` (checksum `b63f63e98f205c13`,
all four combinations identical). This is the strongest evidence for either triple in the whole
investigation, and it also revealed a new, better oracle: **exact PCM/checksum equality between two
fresh-process single-flip-decodes is a threshold-free, drift-free membership test** -- no dB metric,
no in-connection baseline, and no exposure to the busy-history degradation problem, since each test
is exactly one decode per fresh connection.

**Consequence: the full exhaustive sweep needed re-running with proper silence conditioning**
(`examples/p25_ratet27_pairflip_full_sweep.rs`, updated in place), since the unconditioned version
demonstrably missed at least one real structural relationship. Two more tuning problems were found
and fixed before a trustworthy run launched:

1. **Baseline/conditioning mismatch.** An early version of the conditioned sweep captured its own
   baseline via the *old* plain tone-only re-priming method while every subsequent test used full
   silence conditioning -- comparing two different decoder states. This inflated the measured noise
   floor to ~21 dB, which would have set a 5x-floor threshold (~106 dB) far above even the confirmed
   real effects (~10-14 dB), silently missing everything. Fixed by conditioning the baseline capture
   identically to every other test.

2. **A genuinely wide, heavy-tailed noise floor, not a bug.** Even after that fix, 20 independent
   conditioned samples of "decode unmodified R" spread smoothly from 0.5 dB to over 20 dB (not a
   rare-outlier pattern -- a real, continuous spread). Neither increasing the tone-priming count
   (4->20) nor the silence-conditioning count (20->80) meaningfully tightened this, ruling out
   "insufficient relock time" as the cause; a Hann window (to reduce phase-dependent spectral
   leakage, the other likely explanation) changed the distribution's shape but didn't clearly help
   either, and was reverted rather than chased further. This means a single dB-spectral-distance
   measurement cannot, by itself, reliably separate a weak real effect from noise here -- the
   previously-confirmed but weak `(128, 139)` pair (5.56 dB) sits within the same range as ordinary
   noise spikes. **Fix**: use the *median* of 20 calibration samples as a robust floor estimate (not
   swayed by the tail), a modest additive threshold margin above it (low enough to admit real
   10+ dB effects into confirmation, accepting that this also admits much of the noise tail), and a
   mandatory independent-redraw confirmation retest for every candidate -- a genuine effect
   reproduces reliably, noise usually doesn't land above threshold twice. This is an accepted,
   documented limitation for weak effects specifically, not a blocker for the strong ones this sweep
   is actually aimed at finding.

**A striking, highly regular preliminary pattern**, seen in a bounded sanity check before launching
the full run: for anchor bit 0, confirmed hits land at `(0,17) (0,18) (0,19)`, `(0,43) (0,44) (0,45)`,
`(0,69) (0,70) (0,71)`, `(0,95) (0,96) (0,97)`, `(0,121) (0,122) (0,123)` -- five groups of exactly 3
*consecutive* wire positions, each group spaced exactly 26 positions apart, with the same +26 pattern
repeating (from a different starting offset) for anchor bit 1.

**The completed run found 1147 confirmed hits across all 142 candidates (one single connected
component, not isolated small groups) -- and then failed its own sanity check.** Checking whether
the two already-confirmed real triples appeared in this dataset, `(8, 92)`/`(8, 127)`/`(92, 127)`
and `(68, 103)`/`(68, 127)`/`(103, 127)`, found only one match at all: `(68, 103)` appears, but as a
**rejected, unconfirmed** candidate (`first=19.19 dB, retest=5.25 dB`) -- the sweep's own
confirmation step threw out a pair independently verified, multiple times, by direct targeted
testing to be real. A result set that rejects a known-real effect while accepting 1147 others cannot
be trusted as a whole; the 1147-pair dataset (including the period-26 pattern) is **not** treated as
reliable evidence of real structure, despite its superficially compelling regularity.

**Root cause, per Bruce: AMBE's encoder incorporates feedback from previous decoding.** This session
had already independently found and documented (during the AMBE+2 real-speech validation, and again
during the DVSI pipelined-protocol investigation) that this chip's own real client software runs a
genuine multi-frame lookahead/delay pipeline -- Bruce's comment connects that same mechanism to this
specific failure. Verified directly and reproducibly (`examples/p25_ratet27_pairflip_diagnose_hit.rs`,
`condition()`'s own doc comment has the full detail): a single prior flip-decode test already
degrades the *next* test's measured `(68, 103)` effect from 12.41 dB to 5.26 dB, even with the same
conditioning re-applied before it; 100 busy prior flip-decodes plus that same conditioning still only
give 4.03 dB. Appending a second, smaller pass -- 3 more silence decodes, then a few more tone
decodes -- after the main conditioning recovers this specific degraded case back to 11.03 dB,
reproducibly (identical across repeated fresh-process runs). **But this is not a general fix**:
applying that same second pass unconditionally to *every* conditioning call, including an already-
fresh one, was tried and made things worse -- a known real effect measured 8.56 dB while a known
no-effect pair measured 9.19 dB under otherwise-identical conditions (the ranking flipped). Both
variants are individually fully deterministic; this is real, parameter-sensitive state-dependence,
not measurement noise.

**Where this leaves the RATET(27) investigation**: short, targeted, few-tests-per-connection
verification (used throughout §§13-15's actual confirmed findings -- the two redundancy triples, the
two unprotected `c7` bits) remains reliable, since a connection's history stays short and controlled.
A long, exhaustive, thousands-of-tests-on-one-connection sweep does not currently have a trustworthy
conditioning recipe, since no single procedure has been found that produces a canonical state
regardless of a connection's prior history -- confirmed, not just suspected, by the direct
reproduction of degradation-then-partial-recovery above. This is an honest, open limitation of the
exhaustive-sweep methodology specifically, not a retraction of anything already confirmed by targeted
testing. Recovering a fully general, history-independent reset procedure -- or abandoning long-
running sweeps in favor of many short, independently-conditioned connections (fresh RATEP
configuration per test, accepting the added per-test connection-setup cost) -- is the next step for
whoever continues this investigation.

## 16. Sweeping the busy-history recovery frame count, per Bruce's suggestion -- no simple rule found, but a clean protocol conclusion

Per Bruce: "You might try sweeping the number of silence frames to determine how many are
consistently effective," refined by "There need to be enough silence frames to prime both the
encoder and decoder. That may be 6 rather than three." The §15 recovery pass (3 silence decodes +
`PRIME_REPEATS` tone decodes, appended after normal conditioning) was tested only at N=3; this
follows up by varying N.

**Methodological trap found first**: an initial sweep tool (`p25_ratet27_recovery_frame_count_sweep.rs`)
ran multiple (busy-history, recovery, measure) trials back-to-back within *one* continuous
connection. This produced high variance *within* a single N's own repeated trials (e.g. N=0:
`[4.28, 4.80, 10.51]` dB) -- proof that sequential trials sharing one connection do not share a
common baseline; each trial's starting state depends on all the prior trials in that same session,
not just the fixed busy-history replay before it. Confirms again (see §15) that only a genuinely
fresh process/connection per measurement is a trustworthy protocol here.

**Fixed with `p25_ratet27_recovery_n_single_trial.rs`**: exactly one measurement per fresh process,
intended to be invoked repeatedly from a bash loop. Getting this tool to reproduce the known-good
11.03 dB value at N=3 took two real bug fixes, both instructive about how exact-sequence-sensitive
this chip's state is:

1. Its `condition()` was missing the encoder-silence pass (`send_speech_get_channel` over live
   silent PCM) that `p25_ratet27_pairflip_diagnose_hit.rs`'s `condition()` always included --
   without it, N=3 measured 3.81-6.13 dB, not 11.03.
2. `diagnose_hit.rs`'s actual sequence runs the fixed-seed busy history in **two separate rounds of
   100 flips each**, with a `condition()` call and an (unused-for-the-final-result) intermediate
   measurement between them, and the LCG state **continues** across both rounds rather than
   resetting -- the 11.03 dB value was measured after 200 cumulative busy flips, not 100. Matching
   this exactly was what finally reproduced 11.03 dB precisely.

**Swept N in {0, 1, 2, 3, 4, 5, 6, 8, 10}, both `mode=decode_only` and `mode=both` (extra live
encoder-silence frames during the recovery pass itself), 2 fresh-process trials each, target pair
`(68, 103)`:**

| N  | distance (dB), both modes identical |
|----|----|
| 0  | 5.64 |
| 1  | 4.11 |
| 2  | 6.36 |
| 3  | 11.03 |
| 4  | 5.06 |
| 5  | 5.28 |
| 6  | 9.88 |
| 8  | 26.79 |
| 10 | 4.39 |

Three findings, all solid:

- **Fresh-process determinism is exact**: both trials at every single N produced byte-identical
  distances (and, on spot-check, identical PCM). This reconfirms fresh-process single-measurement as
  fully deterministic and reliable.
- **`mode` makes zero difference at every N.** Extra live encoder-silence frames during the small
  recovery pass changed nothing, because `condition()` (run right before the recovery pass in every
  trial) already does 20 rounds of encoder-silence priming -- Bruce's "prime both paths" requirement
  is already satisfied by `condition()` itself, so the recovery pass only ever needed to address the
  decoder side.
- **No N gives consistent recovery.** The relationship is non-monotonic (5.64, 4.11, 6.36, 11.03,
  5.06, 5.28, 9.88, 26.79, 4.39) with no threshold-like "N or more works" structure, and N=8's 26.79
  dB *exceeds* the undisturbed fresh-boot reference (~12.41 dB) -- proof that this distance-from-a-
  fixed-baseline metric, after 200 busy flips plus N recovery frames, is measuring some mix of
  decoder-state drift and the flip effect, not a clean "recovered vs not" signal. There is no simple
  N to recommend.

**Conclusion for the exhaustive-sweep question**: don't chase a general busy-history recovery
recipe further. The only protocol shown reliable all night is a single measurement from a fresh
process; that is the actual answer to "how do you get a trustworthy reading on this chip," not a
particular recovery frame count layered on top of a long, busy connection.

## 17. Weight-3 Hamming-codeword interleave search -- a 4-of-5 near miss, cleanly falsified

Following §15's correction (weight-3 Hamming(15,11) codewords, not majority vote), the confirmed
constraint "{8, 68, 92, 103, 127} all share one 15-bit Hamming block" is far more discriminating than
§14's 2-bit `c7`-pitch-LSB check (which found no matching convention at all). This is a zero-chip-time
search: `p25_ratet27_hamming_block_convention_search.rs` re-uses the same convention space (TIA-
102.BAAA-A Table 5-1 OTA-interleave hypotheses -- dibit-row swap, index direction, byte order, bit
direction -- plus natural contiguous-codeword order) and checks whether any convention places all
five confirmed bits in the same `TIA_BLOCK` (4, 5, or 6).

**No convention places all five together.** But one convention -- `dibit_swap=true,
reverse_bytes=true, lsb_first=false` (index-direction either way, since it only changes offset within
a block) -- places **four of five** (68, 92, 103, 127) in the same block (`TIA_BLOCK`=6), with only
bit 8 landing elsewhere (`TIA_BLOCK`=3, a 23-bit Golay block).

**Falsified directly, decisively, using the new fresh-process checksum oracle (§15's `e5b5...`/
`b63f...` re-confirmation, generalized here as `p25_ratet27_hamming_block_falsification_test.rs`)**:
if bit 8 really sat in a Golay(23,12) block (minimum distance 7), a 2-bit error there could never be
"corrected" onto a third bit the way Hamming(15,11) allows, so `flip{8,92}` and `flip{8,127}` should
each land back near the unmodified baseline while `flip{92,127}` alone shows the large deviation.
Instead, run from independent fresh processes (not sequential same-connection decodes, which by this
point in the investigation was known to be unreliable -- exactly the failure mode this test was
designed to rule out), `flip{92,127}`, `flip{8,92}`, `flip{8,127}`, and `flip{8,92,127}` all produced
byte-identical PCM (checksum `e5b52df714b6e2ad`). The triple is genuinely real (not a same-connection
artifact), and this specific convention is dead.

**What survives**: the hard constraint itself -- whatever the real interleave turns out to be, bits
{8, 92, 127} share one Hamming block and {68, 103, 127} share one (the same one, since 127 is
common), so all five of {8, 68, 92, 103, 127} are in a single 15-bit block. An anchor sweep using the
same fresh-process checksum oracle (bit 127 against all 143 other wire positions, one fresh process
per candidate) was launched to find the block's full 15-bit membership and complete weight-3-codeword
structure directly from the chip, without needing a candidate interleave table at all -- see §18 for
its result.

## 18. Complete membership of one full Hamming(15,11) FEC block, found by pure chip-oracle experiment

The 4-of-5 near miss in §17 motivated a direct, assumption-free sweep: hold bit 127 flipped, and
additionally flip every one of the other 143 wire positions in turn (`flip{127, b}` for
`b in 0..144, b != 127`), one fresh process per `b`, comparing the resulting PCM checksum against the
unmodified-baseline checksum. **16 of the 143 candidates produced a non-baseline checksum -- 2 of
which are the already-known unprotected `c7` pitch bits, 131 and 143** (per §14; confirmed here as
`flip{131}` and `flip{143}` alone reproduce the exact same checksum as `flip{127,131}`/`flip{127,143}`,
validating that an out-of-block bit paired with 127 shows its own independent effect, not a null one).
That leaves **exactly 14 real block members** -- the expected count of "other members" a 15-bit
Hamming(15,11) block should have relative to one anchor. This gives a complete, empirically-derived
membership map for one entire FEC block, with zero assumptions about interleave, byte order, or any
candidate table:

**{8, 20, 32, 44, 56, 68, 80, 92, 103, 104, 115, 116, 127, 128, 139}** (15 positions).

The 14 non-anchor members grouped into 4 distinct checksum classes:

| checksum (dB effect) | members |
|---|---|
| `e5b52df7...` (10.62 dB) | 8, 92 |
| `55ce58d8...` (13.95 dB) | 20, 32 |
| `b63f63e9...` (12.41 dB) | 44, 56, 68, 80, 103, 104, 115, 116 |
| `3985aaa9...` (6.75 dB) | 128, 139 |

**Two new triples, `{20, 32, 127}` and `{128, 139, 127}`, were independently confirmed with the same
full rigor as §15's original two** (`p25_ratet27_hamming_block_falsification_test.rs`, fresh process
per test): all four combinations -- both pairs, both singles-with-127, and the full triple -- produce
byte-identical PCM within each group. Bit 32, previously seen only as an unverified hint from the
disqualified full-exhaustive sweep (§15) and dropped in §17, is now independently confirmed real --
it pairs with 20, not with 127 alone or with the original {8,92,103} group.

**The 8-member class is not one pair -- it is 4 distinct genuine codewords that happen to be
audibly indistinguishable on this test signal, and all 4 were found directly.** Testing
`flip{56, 80}`, `flip{56, 115}`, `flip{56, 116}` (fresh process each) found exactly one match,
`flip{56, 80}` -> the `b63f...` checksum; testing the resulting elimination pair directly,
`flip{115, 116}`, gave `b63f...` too. Both `{127, 56, 80}` and `{115, 116, 127}` were then confirmed
with the same full 2-of-3/3-of-3 rigor as every other triple in this investigation. This resolves the
8-member class completely into 4 real pairs: **{68, 103}, {44, 104}, {56, 80}, {115, 116}** -- all
four, combined with anchor 127, produce byte-identical PCM despite being 4 genuinely distinct
codewords. This directly parallels §14's `c7` finding (5 of 7 raw bits showed no audible effect on
this same test signal): most of this Hamming block's 11 information bits are apparently inaudible or
carry a redundant/overlapping effect for a steady tone, and only the few responsible for the
10.62/13.95/12.41/6.75 dB effects are perceptible with this specific test material.

**A striking arithmetic pattern, found after the fact by inspecting the 15 confirmed positions**: 11
of them -- {8, 20, 32, 44, 56, 68, 80, 92, 104, 116, 128} -- are all congruent to 8 (mod 12); the
other 4 -- {103, 115, 127, 139} -- are all congruent to 7 (mod 12). Since `144 = 12 x 12`, this is
consistent with the chip's raw serial data being organized as a 12-column grid (wire position
`= 12*row + column`, `row` and `column` both 0..11), with this Hamming(15,11) block drawing its 11
higher-weight bits from column 8 (rows 0-10, missing row 11 = position 140) and its remaining 4 bits
from column 7 (rows 8-11). The two confirmed unprotected `c7` pitch bits, 131 and 143, are also both
congruent to 11 (mod 12) -- consistent with column 11, rows 10-11. This "12-column interleave"
hypothesis is a genuinely new, well-defined, falsifiable candidate structure, unrelated to the
TIA-102.BAAA-A Table 5-1 conventions tried and ruled out in §17. An anchor sweep on a column-9
candidate bit (e.g. anchor 9) was launched to test it directly: if a second Hamming block's ~14
members also fall on a clean stride-12 pattern, that would be strong independent confirmation of both
the block-2 membership and the whole 12-column layout -- see the addendum below (or a future section)
for its result.

**This is, along with the confirmed unprotected `c7` pitch bits (§14), the most complete real
structural result this investigation has produced for RATET(27)**: one full 15-bit FEC block's exact
wire-bit membership (with all 7 of its internal weight-3 codewords involving anchor 127 accounted
for: 4 giving one audible signature class, plus the {8,92}/{20,32}/{128,139} classes -- 3+4=7 pairs
among the 14 non-anchor members, matching the theoretical count exactly), obtained by pure black-box
chip experimentation with no assumptions about interleave, byte order, or any candidate specification
table, plus a new candidate global interleave structure (12-column grid) actively being tested. It
remains an open question which of the chip's 3 total Hamming(15,11) blocks this is, and where the
other 2 Hamming blocks and 4 Golay(23,12) blocks land among the remaining 129 wire positions --
continuing the anchor-sweep technique on other columns (per the stride-12 hypothesis) is the natural
next step for whoever continues this investigation.

## 19. A second Hamming block predicted by the transform, hidden from the pure-tone oracle, confirmed with a richer test signal

§18's stride-12 observation was extended (analytically, zero chip time) into a specific transform
hypothesis: the wire (transmitted) format is a 12x12 block interleaver over a natural bit order where
the wire position `m` corresponds to natural position `n(m) = 12*(m mod 12) + (m div 12)` (a matrix
transpose: write the natural stream down 12 columns, read it back out across 12 rows). Applying this
transform to the confirmed 15-member block's own wire positions gives natural positions
**92 through 106, exactly 15 consecutive values** -- strong support for the transform itself, since
only this specific write-by-column/read-by-row structure would keep a real FEC block's bits
contiguous in natural order after this kind of interleaving.

Extending this with the further guess that the 8 FEC sub-blocks are simply concatenated in natural
order as `u0..u3` (Golay x4, 23 bits each, natural 0-91), `u4..u6` (Hamming x3, 15 bits each, natural
92-136), `u7` (raw, 7 bits, natural 137-143) -- matching the confirmed block to `u4`, and noting this
is the *only* way to partition 144 into `[23,23,23,23,15,15,15,7]` consistent with `u4` at 92-106 and
`c7` at 137-143 (the latter independently confirmed: `natural(131)=142`, `natural(143)=143`, both
inside 137-143) -- predicts the *next* Hamming block `u5` at natural 107-121, transforming back to
wire positions **{9, 10, 21, 22, 33, 45, 57, 69, 81, 93, 105, 117, 129, 140, 141}**.

**First attempt looked like a clean falsification, and was not.** An anchor sweep on wire bit 9 with
the usual 200 Hz sine (one fresh process per candidate, same technique as §18) found no partners at
all among the predicted `u5` set -- only `flip{9,131}` and `flip{9,143}` showed any effect (the
already-known unprotected `c7` bits, which register regardless of what else is flipped). Follow-up
pairwise tests within the predicted set (`{9,21}`, `{9,10}`, `{21,22}`, `{10,22}`, `{9,33}`,
`{9,141}`), a 4-flip (`{9,21,33,45}`), and a 5-flip (`{9,21,33,45,57}`) were *all* null too.

**But an all-null result under every flip pattern does not discriminate between competing
explanations, and this session had already shown why**: §18's own confirmed `u4` block has null-
under-pairwise-flip members too (`{44,80}`, `{44,115}`, `{44,116}`), simply because those specific
codewords happen to change something the 200 Hz sine doesn't render (a pure tone puts energy in one
harmonic; most of IMBE's per-band amplitude bits sit at the noise floor for it, so their codewords
are audibly inert). An all-null column-9 sweep is equally consistent with "Hamming block with 7
tone-inaudible codewords," "Golay block with a tone-inaudible weight-7 codeword," or "column 9 spans
multiple blocks" -- the oracle itself was blind, not necessarily the hypothesis wrong.

**Fix: switch to a harmonic-rich reference signal.** Added a 200 Hz sawtooth alternative (same
fundamental, so pitch/voicing structure carries over, but energy spread across many harmonics
instead of one) to `p25_ratet27_hamming_block_falsification_test.rs` (`--signal sawtooth`). Verified
determinism first (two fresh processes, `flip{0,0}`, identical checksums) and re-verified the
confirmed `u4` triple `{8,92,127}` still holds (all four combinations byte-identical under the new
signal). Then re-tested the predicted `u5` pairs: **`flip{9,21}` and `flip{9,10}` produce
byte-identical PCM, while `flip{21,22}` and `flip{10,22}` each produce their own distinct non-null
effect** -- exactly the structure the pure tone was blind to. **This confirms the transform and the
natural-order prediction: `u5` is real.** A full anchor-9 sweep with the sawtooth signal was launched
to map its complete membership the same way `u4` was mapped in §18 -- see the next section for its
result.

Separately (zero chip time): layering the same 12x12 transform onto Table 5-1's own row numbering,
in both orders (transform-then-byte-convention and byte-convention-then-transform) across all 4
byte/bit-order hypotheses, still does not place the confirmed 15-member block in one Hamming slot
together with both known `c7` bits in block 7 -- reinforcing that this chip's host-interface
"channel" packing is a genuinely different, proprietary format, not Table 5-1 plus a simple
transpose layer.

**Lesson for the rest of this investigation**: a null result from the 200 Hz sine test signal is not
reliable evidence of "no relationship" -- it only shows no relationship *audible on a pure tone*. Any
future anchor sweep that comes back all-null should be re-run with the sawtooth signal before being
treated as a real negative.

Separately (zero chip time): layering the same 12x12 transform onto Table 5-1's own row numbering,
in both orders (transform-then-byte-convention and byte-convention-then-transform) across all 4
byte/bit-order hypotheses, still does not place the confirmed 15-member block in one Hamming slot
together with both known `c7` bits in block 7 -- reinforcing that this chip's host-interface
"channel" packing is a genuinely different, proprietary format, not Table 5-1 plus a simple
transpose layer.

## 20. Second Hamming block mapped, and the complete 7-bit `c7` raw block found -- the transform validated on 3 of 8 sub-blocks with zero mismatches

The full anchor-9 sweep, re-run with the sawtooth signal, resolved cleanly into exactly the
predicted structure. 143 candidates grouped into: the dominant null class (122 members, checksum
`b65c2ca8...`), **7 matched pairs** (14 positions), and **7 unrepeated singles** (7 positions) --
21 non-anchor positions total, splitting neatly into two different phenomena:

**The 7 matched pairs give `u5`'s complete membership, exactly matching the transform's prediction
with zero mismatches:**

| checksum | members |
|---|---|
| `1f101d45...` | 10, 21 |
| `defe0d99...` | 22, 93 |
| `bdf17b41...` | 33, 117 |
| `7d4a72ec...` | 45, 57 |
| `0810d1b9...` | 69, 129 |
| `cd9c8c56...` | 81, 105 |
| `78ed0fe7...` | 140, 141 |

Combined with anchor 9: **u5 = {9, 10, 21, 22, 33, 45, 57, 69, 81, 93, 105, 117, 129, 140, 141}** --
identical, position for position, to the wire set §19 derived analytically from the transform
(`natural(m) = 12*(m mod 12) + (m div 12)` applied to natural range 107-121). Zero mismatches.

**The 7 unrepeated singles are the complete 7-bit unprotected `c7` block, not just the 2 pitch bits
found in §14.** `{71, 83, 95, 107, 119, 131, 143}` each showed their own distinct, non-repeating
effect (consistent with raw/unmodulated bits: each shows its own independent signature regardless of
what else is flipped, rather than pairing up like FEC-protected bits do). These 7 positions are all
congruent to 11 (mod 12), stride-12, and map under the transform to **natural positions 137 through
143 -- exactly 7 consecutive values**, matching §19's `c7`-placement prediction exactly. The other 5
of these 7 (everything except the already-known 131, 143) were invisible to the original single-bit
oracle (§14) for the same reason column 9's Hamming partners were invisible to the anchor-9 sine
sweep: a pure 200 Hz tone doesn't render whatever these bits control (very likely low-order spectral-
amplitude LSBs, per §14's original textbook-count reasoning -- they were never really "inert," just
inaudible on that specific signal).

**The transform is now validated on 3 of 8 sub-blocks (`u4`, `u5`, `c7`) with zero mismatches
between prediction and direct chip measurement.** The remaining 5 sub-blocks (`u6`, and 4 Golay
blocks `g0..g3`) have fully determined predicted wire-position sets from the same transform (natural
ranges 122-136 for `u6`, and 0-22/23-45/46-68/69-91 for `g0..g3`):

- predicted `u6` = `{11, 23, 34, 35, 46, 47, 58, 59, 70, 82, 94, 106, 118, 130, 142}`
- predicted `g0` = `{0, 1, 12, 13, 24, 25, 36, 37, 48, 49, 60, 61, 72, 73, 84, 85, 96, 97, 108, 109, 120, 121, 132}`
  (and `g1`/`g2`/`g3` follow the same pattern shifted by natural offsets 23/46/69)

A quick spot-check of 4 predicted `u6` pairs (`{11,23}`, `{34,35}`, `{11,35}`, `{23,34}`, sawtooth
signal, fresh process each) found all 4 non-null with distinct checksums -- consistent with `u6`
being real (not yet full-membership-mapped; a complete anchor-11 sweep was launched the same way as
`u4`/`u5` to confirm it fully -- see the next section for its result).

## 21. Third Hamming block confirmed with zero mismatches, and all 4 Golay-block boundaries confirmed directly

The anchor-11 sawtooth sweep (predicted `u6`, per §20's transform) resolved into exactly the
predicted structure, same as `u4` and `u5`: 7 matched pairs plus the same 7 `c7` singles.

| checksum | members |
|---|---|
| `64fc65ff...` | 23, 118 |
| `4aaab9b1...` | 34, 94 |
| `5c0a77b5...` | 35, 82 |
| `5d26390b...` | 46, 70 |
| `ca72415a...` | 47, 59 |
| `72f4ab71...` | 58, 130 |
| `d58854aa...` | 106, 142 |

Combined with anchor 11: **u6 = {11, 23, 34, 35, 46, 47, 58, 59, 70, 82, 94, 106, 118, 130, 142}**
-- identical, position for position, to §20's transform prediction (natural range 122-136). Zero
mismatches, the third Hamming block in a row.

**A Golay(23,12) block was then directly confirmed too**, using the predicted `g0` membership
(`{0, 1, 12, 13, 24, 25, 36, 37, 48, 49, 60, 61, 72, 73, 84, 85, 96, 97, 108, 109, 120, 121, 132}`,
natural range 0-22): a 3-bit in-block flip (`{0,1,12}`) is null (Golay's minimum distance of 7
corrects any error of weight <=3 perfectly, exactly reproducing the original), while two *different*
4-bit in-block flips (`{0,1,12,13}` and `{0,1,24,25}`) each produce their own distinct non-null
effect -- exactly the Golay signature predicted in §18's original advisor review, now confirmed
directly against the chip for the first time this entire investigation.

**All 4 Golay-block boundaries were then confirmed directly, per advisor review** (a single in-block
Golay signature isn't proof of the *exact* predicted membership, since a null 3-flip is also what
three bits scattered across three different blocks would produce). For each adjacent pair of
predicted Golay blocks, one 4-flip fully inside the first block was compared against one 4-flip
straddling the boundary (2 bits from each side): `g0`/`g1` (`{0,1,12,13}` non-null vs `{0,1,2,3}`
null), `g1`/`g2` (`{2,3,14,15}` non-null vs `{2,3,4,5}` null), `g2`/`g3` (`{4,5,16,17}` non-null vs
`{4,5,6,7}` null) -- every straddle is null (two independent 2-bit errors, each within its own
Golay block's 3-error correction radius, each self-corrects back to the original) while every
in-block 4-flip is non-null, confirming each boundary exactly. `g3`'s own in-block signature
(`{6,7,18,19}`) was null on the first three subsets tried (`{6,7,18,19}`, `{6,7,30,31}`,
`{6,7,42,43}`, `{18,19,30,31}`) -- consistent with §18's already-established finding that many real
codewords are inaudible on any given test signal, not evidence against `g3` -- and a wider-spread
subset (`{7,55,79,113}`, matching `{6,7,113,114}`'s checksum) confirmed a real, audible `g3`
codeword. All 4 Golay blocks are now structurally confirmed, completing direct verification of
every one of the 8 FEC sub-blocks' existence and boundaries (4 fully bit-mapped, 4 boundary-and-
signature confirmed).

**Primary-source confirmation, found while researching a separate question from Bruce about whether
this chip supports a voice/data split mode (it does not -- see the answer recorded below)**: DVSI's
own AMBE-3000R Vocoder Chip Users Manual (Section 6.9, CHAND field description) states directly:
*"Chand[0] contains the bits which are most sensitive to bit errors... Chand[(Bits-1)/8] contain the
bits which are least sensitive to bit errors."* This is DVSI's own documented design principle
matching, exactly, the natural-order assumption this investigation inferred empirically (4 Golay
blocks first -- Golay is the strongest protection, minimum distance 7 -- then 3 Hamming blocks --
minimum distance 3, weaker -- then the unprotected raw `c7` bits last). What was an inferred
convention that happened to fit the data is now a documented DVSI design principle independently
corroborating it.

**Where this leaves the RATET(27) wire format**: of the 8 total FEC sub-blocks, 4 are now fully,
exactly mapped by direct chip experiment (`u4`, `u5`, `u6`, `c7` -- 52 of 144 wire positions with
zero prediction errors across all of them), and all 4 Golay blocks (`g0`-`g3`, 92 more positions)
have their exact predicted boundaries directly confirmed via the in-block-vs-straddle 4-flip test
above, though their individual bit-for-bit membership (unlike the Hamming blocks) hasn't been
walked position-by-position the way an anchor sweep would. The transform itself: **wire position
`m` corresponds to natural (pre-interleave) position `12*(m mod 12) + (m div 12)`**, with the
natural bit stream simply being the 8 FEC sub-blocks concatenated in decreasing-protection order
(`g0..g3` at natural 0-91, `u4..u6` at natural 92-136, `c7` at natural 137-143) -- a full,
falsifiable, and now extensively chip-verified model of this chip's real host-interface wire format,
found entirely by black-box experimentation with zero access to DVSI's proprietary interleave
specification.

**One open question checked and left genuinely unresolved**: §19 flagged that under the transform,
`131`->natural 142 and `143`->natural 143 (`c7` offsets 5 and 6), which doesn't cleanly match this
crate's own `extract_fundamental_frequency_quantizer` formula (`(u[7]>>1)&0b11`, wanting `c7` bits 1
and 2) under either an MSB-first or LSB-first storage convention -- under MSB-first, that formula
would instead want offsets 4 and 5 (wire 119 and 131). Tested directly: `flip{107}`, `flip{119}`,
`flip{131}`, `flip{143}` each alone (sawtooth signal) all show their own distinct, roughly
comparable non-null effect (4.5-5.7 dB range) -- no clear "two strong pitch-class bits vs two weak
amplitude-class bits" split emerged that would discriminate which pair is really the pitch field
under this chip's actual bit-index convention. This remains an open, unresolved constraint on any
final decoder for this chip's `c7` field; the original finding that only `131`/`143` (not the other
5) show any effect under a *pure tone* still stands as the most specific evidence available, but
doesn't by itself resolve the formula's exact bit-index convention.

**A capability found but not used tonight, for whoever continues this**: the manual documents
`CHAND4` (field ID `0x17`), a soft-decision decode mode (4-bit confidence values, 2 per byte,
instead of hard 0/1 bits). A future investigator could feed maximal-uncertainty ("don't know")
soft values to every bit outside the block currently under test, which may suppress cross-block
miscorrection noise entirely rather than needing to reason about it after the fact -- not tried
this session, but a real, documented feature worth exploring.

**Answer to Bruce's question: does this chip support a "data mode" or voice/data split, separate
from the vocoder's own compressed voice bits?** No. A careful read of both DVSI's USB-3000 Family
manual and the full AMBE-3000R Vocoder Chip Users Manual found: only 3 packet types exist
(`CONTROL`, `CHANNEL`, `SPEECH`); the channel packet's only data-carrying fields are `CHAND`/`CHAND4`
(compressed voice bits, hard/soft-decision), `SAMPLES`, `CMODE`, and `TONE`; and the encoder/decoder
feature-flag words (`ECMODE_IN`, `DCMODE_IN`/`DCMODE_OUT`) are entirely audio-processing toggles
(noise suppression, echo cancellation, companding, DTX/silence detection, tone detect/send, frame
repeat, comfort noise) with no data-channel concept anywhere. Whatever "data" a real over-the-air
protocol carries alongside voice (P25's embedded Link Control/Low Speed Data, D-STAR's slow-data
field) is added entirely by the surrounding radio/modem framing, external to the vocoder chip -- the
chip's own channel bitstream is 100% voice+FEC, confirming there is no host-exposed pass-through path
that would have let this investigation bypass vocoder synthesis directly (the sawtooth-signal fix in
§19/§20 remains the real, practical workaround for that problem).

## 22. Closing the loop: the original 6-hit pair sweep (task `b6tc68au3`, §15) cross-validated against the now-complete block map

Per a direct request to check on this task: it completed hours before this session's §17-21 work
even began (833.2s runtime, exit code 0, output already on disk), and its 6 confirmed hits --
`(8,92)`, `(8,127)`, `(32,127)`, `(68,103)`, `(103,127)`, `(128,139)` -- are exactly what §15 already
documented and analyzed at the time. That analysis is not being redone here; what's new is checking
those 6 pairs against the fully-mapped 8-block structure §§18-21 established using a completely
different, more reliable methodology (fresh-process checksum equality instead of dB-threshold
sweeping on one long-lived connection).

**Every one of the 6 hits falls entirely within `u4`'s exact confirmed 15-member set**
(`{8, 20, 32, 44, 56, 68, 80, 92, 103, 104, 115, 116, 127, 128, 139}`) -- both positions of all 6
pairs are `u4` members, with zero exceptions. This is a clean, independent cross-validation: a
dataset collected hours earlier, with a since-shown-unreliable methodology (dB-threshold, no
sawtooth signal, single long connection), landed 100% inside the block this session went on to map
completely and independently through a different technique.

**It also retroactively explains this session's central obstacle.** The old sweep tested all
C(142,2) pairs across the *entire* 144-bit frame but found real effects *only* within `u4` -- not
because the other 7 sub-blocks (3 more Hamming blocks' worth of pairs, 4 Golay blocks' worth, and
`c7`'s own raw bits) don't exist or don't matter, but because none of their codewords happened to be
audible on the plain 200 Hz sine test signal that sweep used. This is precisely the "pure tone is
blind to most of the wire format" problem diagnosed and fixed in §§19-20 with the sawtooth signal,
visible in hindsight all the way back in this much earlier dataset. `u4` was simply the one block
whose confirmed codewords all happened to be audible on a sine tone -- the lucky block, not a
special one -- which is exactly what let this whole investigation get started in the first place
(§15) before the real, larger structure came into view (§§17-21).

## 23. The labeling question is provably unsolvable by relational testing alone -- pivoting to direct chip-frame sampling resolves 7 of 8 FEC blocks completely, with a real software duplicate now committed

Following the `/goal` directive to continue until the RATET(27) format is fully known and duplicated
in software, this session attempted to resolve the one remaining open question from §21: given a
block's confirmed wire-bit *membership* (now complete for all 8 sub-blocks), what is the actual
bit-index-within-codeword *permutation* -- i.e. which physical wire bit is `fec.rs`'s codeword bit
14 vs. bit 3 vs. bit 0?

**This turned out to be mathematically unsolvable by any amount of black-box weight-3/weight-7
relational testing, proven directly rather than merely suspected.** A brute-force Python constraint
solver was built representing each wire position as an unknown column vector in GF(2)^4 (matching
`fec.rs`'s own `HAMMING_PARITY` column structure), with one constraint per confirmed weight-3
codeword (`column(a) XOR column(b) XOR column(c) = 0`). Using all 18 of `u4`'s confirmed triples
(three independent anchors: 127, 8, 92 -- the latter two swept fresh specifically to break the
degeneracy an earlier single-anchor attempt hit), the solver still found over 20,000 consistent
candidate permutations (search capped there for runtime, but the true count is almost certainly
much higher). Merging in `u5`'s and `u6`'s own 7 triples each (32 total constraints across three
independently-derived Hamming blocks) made no difference -- still capped at 20,001 solutions.

**The reason is structural, not a data shortage**: every constraint of the form `col(a) XOR col(b)
XOR col(c) = 0` is invariant under *any* linear automorphism `T` of GF(2)^4 applied simultaneously
to every column, since `T(col(a)) XOR T(col(b)) XOR T(col(c)) = T(0) = 0` for any linear `T`. The
Hamming(15,11) code's automorphism group has order `|GL(4,2)| = 20160`; the Golay(23,12) code's is
larger still (order roughly 10^7, related to the Mathieu group `M23`). No number of anchor bits or
flip-sweep triples can ever break this symmetry -- confirmed here computationally, not just argued
abstractly, and worth recording so no future session re-attempts the same approach expecting more
data to eventually resolve it.

**The fix: stop asking "which physical bit is codeword index N" and instead sample the code
directly.** The chip's own encoder emits real, valid codewords on every live frame. Two new
committed tools do this:

- `examples/p25_ratet27_capture_frames.rs` -- streams a wide variety of stimuli (9 voice-range
  frequencies as both sine and sawtooth, an amplitude ramp, silence, and 60+ pseudo-random LCG-noise
  frames at varying peak amplitude) through the real chip, dumping each response's raw 18-byte
  CHAND payload as a hex line. Includes a retry-with-backoff wrapper around the send/recv round
  trip, since this session had already hit several transient `WouldBlock` UDP timeouts under
  sustained chip load (previously fatal to long-running sweeps).
- `examples/p25_ratet27_capture_noise_burst.rs` -- a focused follow-up: more pseudo-random noise at
  8 fixed peak amplitudes (500 through 16000), for topping up coverage on whichever block needed
  more distinct samples.
- `examples/p25_ratet27_capture_real_speech.rs` -- streams real recorded speech (the already-cleared
  Open Speech Repository fixtures at `tests/fixtures/osr_speech/`, 8kHz mono, this codec's native
  rate) through the chip 160 samples at a time. Added specifically because synthetic noise alone
  plateaued on one block (see below) -- real speech has far richer, non-stationary spectral content
  than any synthetic signal this investigation had tried.

Across all three tools, **~2200 total captured frames, 1490 of them bit-for-bit distinct**, were
deinterleaved via the validated `natural_position` transform and split into the 8 sub-blocks.
Treating each block's own observed natural-order bit patterns as vectors in a GF(2) linear code and
computing GF(2) rank (Gaussian elimination) gives a direct, assumption-free answer to "is this
really an unwhitened FEC codeword, and if so, what's its generator matrix":

| block | rank found | expected (pure codeword) | verdict |
|---|---|---|---|
| `g0` | 12 | 12 | full rank -- pure Golay(23,12) codeword |
| `g1` | 12 | 12 | full rank -- pure Golay(23,12) codeword |
| `g2` | 12 | 12 | full rank -- pure Golay(23,12) codeword |
| `g3` | **8** | 12 | **short -- see below, unresolved** |
| `u4` | 11 | 11 | full rank -- pure Hamming(15,11) codeword |
| `u5` | 11 | 11 | full rank -- pure Hamming(15,11) codeword |
| `u6` | 11 | 11 | full rank -- pure Hamming(15,11) codeword |
| `c7` | 7 | 7 | full rank -- genuinely raw/unprotected, confirms DVSI's own description |

Full rank on 7 of 8 blocks is itself a real, standalone finding: it directly answers a question this
investigation had left open since §17 (whether the chip mixes any data-dependent whitening/PRN into
the wire bits, the way the textbook IMBE encoder's own `modulation` stage does) -- **it does not**,
for every block reaching full rank. The wire bits genuinely are the FEC codewords themselves.

**`g0`/`g1`/`g2`'s row-reduced generator basis turned out to be bit-for-bit identical to `fec.rs`'s
own systematic Golay(23,12) construction** -- same data/parity split (natural offsets 0-11 as free
data-bit positions, 12-22 as determined parity, an exact match in all 3 x 12 = 36 compared rows),
same `GOLAY_PARITY` values, same bit order (natural offset ascending = codeword bit descending,
MSB-first). This chip's real Golay code needs **no new implementation at all**: `fec.rs`'s existing
`golay_encode`/`golay_decode` apply directly to each Golay block's natural-order bits, verified with
a real round-trip test (`decode_block_round_trips_g0_with_the_real_golay_code`).

**`u4`/`u5`/`u6` all share one identical row-reduced generator basis** (one Hamming FEC routine used
three times, exactly as expected), but this basis's parity submatrix is a **different, though
equally valid, labeling** of the same 11 nonzero 4-bit column values `fec.rs`'s own
`HAMMING_PARITY` uses -- confirmed the columns are the same *set* (`chip_hamming_parity_uses_the_
same_15_nonzero_columns_as_fec_rs`), just assigned to different data-bit positions. This chip-real
table, `HAMMING_PARITY_CHIP = [0b1001, 0b1101, 0b1111, 0b1110, 0b0111, 0b1010, 0b0101, 0b1011,
0b1100, 0b0110, 0b0011]`, was **cross-validated against all 32 of this session's independently
gathered empirical weight-3 relationships** (18 from anchor 127, 7 from anchor 9, 7 from anchor 11 --
the very data the relational-testing approach above proved could never uniquely determine a
permutation) and every single one holds exactly, with zero mismatches. The permutation-degeneracy
result above and this validation are not in tension: many permutations satisfy those 32 relational
constraints, but the *one this session actually derived by sampling real codewords* is confirmed
consistent with all of them, which is the strongest evidence available that it's the chip's real
table (not merely "a" table that happens to work).

**New committed code**: `src/ambe/ratet27_wire_format.rs` (the validated 12x12 transform, the 8
sub-block boundaries, and `block_wire_members` computing each block's exact wire membership
programmatically from the transform rather than as separately hand-maintained lists -- regression-
tested against every directly chip-confirmed set from §§18-21 with zero mismatches) and
`src/ambe/ratet27_fec.rs` (the chip-real Golay reuse of `fec.rs`, the new `HAMMING_PARITY_CHIP`
table and its encode/decode functions, a unified `decode_block` entry point, and the full validation
test suite described above -- 19 tests total across both files, all passing). This is a genuine,
tested, chip-validated software duplicate of RATET(27)'s FEC layer for 7 of its 8 sub-blocks, not
just documentation of the format.

**`g3` remains open, and this is a real finding, not a sampling gap.** Despite the same ~2200-frame,
1490-distinct-frame capture spanning pure tones, 8 different noise amplitudes, and 16 seconds of
real recorded speech, `g3`'s observed wire bits plateau at exactly GF(2) rank 8 (not 12), with 4 of
its 23 natural-order bits (offsets 2-5, transforming to natural positions 71-74) staying **exactly
zero in every single one of the 1490 distinct captured frames** -- not merely rare, literally never
observed as 1. More than 100x the frame count that reached full rank on every other block failed to
move `g3` past rank 8, across wildly different stimulus types, which rules out "just needs more
samples" as the explanation. The most plausible reading is that `g3` carries a parameter tied to
some speech characteristic none of this investigation's stimuli (or, apparently, ordinary read-aloud
English sentences) ever produce -- deliberately extreme pitch, a specific voicing pattern, or very-
high-order spectral content are candidates, but this needs either different stimulus material or a
from-spec understanding of exactly which IMBE parameter lands in the highest-index Golay block to
know what to specifically provoke. `decode_block` deliberately panics if called on `g3` rather than
silently assuming it matches `g0`-`g2`'s already-confirmed generator.

**`g3`'s rank-8 plateau confirmed exhaustively, not just under the original stimulus set.** Three
follow-up capture rounds specifically targeted `g3`: two more real-speech recordings from different
OSR speakers (1000 more frames), and a dedicated exotic-stimulus tool
(`examples/p25_ratet27_capture_exotic_stimuli.rs`) covering 5 dual-tone/DTMF-style two-sinusoid
mixes, 5 fast intra-frame linear chirps sweeping the full pitch range in both directions, and 7
sine tones at and beyond the edges of AMBE's documented 57-444Hz pitch range (down to 30Hz, up to
800Hz). None of it moved the needle even slightly: **2687 total distinct captured frames (spanning
tones, ramps, 8 noise amplitudes, 3 real speakers' worth of recorded English speech, dual tones,
chirps, and extreme pitch) all land in the exact same GF(2) rank-8 subspace**, with the exact same 4
of 23 natural-order bits staying exactly zero throughout. This is about as thorough an audio-domain
stimulus search as is practical, and the finding held with zero exceptions across all of it -- strong
evidence that whatever `g3` encodes is not reachable by any single-frame audio stimulus at all, and
more likely depends on either multi-frame encoder history/adaptive state that builds up over many
frames of specific dynamics (not reachable in a short capture window), or an encoder feature/mode
this investigation's SPEECH-packet-only testing never engages (frame-repeat, DTX, or another
documented `ECMODE`/`DCMODE` flag). Resolving this now needs a from-spec understanding of exactly
which IMBE parameter and bit-history dependency lands in the highest-index Golay block, not more
undirected stimulus variety -- recorded here so a future session doesn't repeat the same broad
audio-stimulus search expecting a different result.

**Two scope notes for whoever continues this work, stated now rather than discovered later:**

- §9's finding that pitch lives in `u2`, Gray-coded, was derived through the *textbook* TIA-102
  Annex H deinterleave -- which this entire investigation (§§15-22 and this section) has since shown
  does not match this chip's real wire format. That specific claim needs re-deriving through the
  correct `ratet27_wire_format` deinterleave plus an empirical decode before anything semantic gets
  built on top of it; it should not be assumed to still hold.
- Byte-exact PCM reproduction of the chip's own *synthesis* output is almost certainly unreachable
  regardless of how completely the FEC/interleave layer above gets nailed down -- DVSI's manual
  documents proprietary post-processing (noise suppression, spectral enhancement variants, comfort
  noise) in the synthesis path that this crate's from-spec `src/ambe/` implementation was never
  going to reproduce bit-for-bit. The realistic, achievable validation bar for the semantic
  (voice-parameter) layer, once attempted, is parameter-level agreement -- decoded pitch/voicing/
  amplitude values matching what this crate's own encoder produces for the same input PCM -- not
  identical output samples. Recording this now so the eventual semantic-mapping work is scoped
  honestly from the start rather than discovering the ceiling at the end.

**A first, concrete probe at the semantic (u-vector) remapping question, negative but informative.**
`ambe::mod.rs`'s `encode_frame` was refactored (no behavior change -- verified by the full 190-test
`ambe` module suite passing unchanged) to expose its own pre-FEC `u_hat_0..u_hat_7` via a new public
`encode_prioritized_bits`, specifically to let real-chip validation work compare this crate's own
textbook `u_hat` semantics against the real chip's decoded data. The natural first hypothesis: since
`bit_prioritization`'s own block widths `[12,12,12,12,11,11,11,7]` exactly match the chip's real
`g0..g3`/`u4..u6`/`c7` sizes and natural order, maybe `u_hat_0` corresponds directly to `g0`'s
decoded data, `u_hat_1` to `g1`'s, and so on, with no further block-level relabeling needed.
`examples/ratet27_compare_textbook_u_vectors_to_chip.rs` tests this directly: for a known-frequency
pure sine, feed the *exact* pitch (not an estimate) through `encode_prioritized_bits` to get this
crate's own `u_hat_0`, and decode a real captured chip frame at the same frequency's `g0` block via
`ratet27_fec::decode_block`. **The top 6 bits did not match at any of 4 tested frequencies
(100/200/250/400 Hz)** -- and, notably, the chip's own decoded `g0` value saturated to the same
all-1s top-6-bits pattern (`0b111111`) at 200, 250, and 400 Hz while showing a distinct value at 100
Hz, which does not resemble the textbook fundamental-frequency quantizer's own smoothly-varying
`floor(4*pi/omega0 - 39)` output for those same frequencies. This rules out the simplest possible
`u0=g0` direct-correspondence hypothesis, at least for the fundamental-frequency field specifically,
and hints that the real chip's own pitch estimation/quantization convention may differ substantially
from the textbook's (not just its bit ordering) -- consistent with, and now a second independent
data point supporting, §9's earlier finding that the textbook's pitch-bit assumptions don't carry
over to this chip. This negative result is recorded so a future session doesn't re-attempt the same
simple hypothesis; the semantic-mapping question remains genuinely open and likely requires
understanding the chip's own pitch estimator (a substantial, separate reverse-engineering task) more
than further trial-and-error block-correspondence guessing.

**A second, broader semantic probe using real speech (not a single assumed-exact pitch), also
inconclusive but with one lead worth following up.** Since this chip's encoder is known to never
converge to a fixed steady state on a pure tone (`examples/ambe_chip_validate_dstar.rs`'s own doc
comment), the single-frequency `u0=g0` test above may simply have compared mismatched effective
pitches. `examples/ratet27_speech_u_vector_correlation.rs` runs real recorded speech through both
paths in lock-step, frame by frame -- this crate's own full encode pipeline (the same per-frame
pitch grid-search as `tests/ambe_real_speech_round_trip.rs`, with `FrameState` history properly
chained across frames) and the real chip on the identical PCM -- dumping each successfully-encoded
frame's `u_hat_0..u_hat_7` alongside the chip's own decoded `g0`/`g1`/`g2`/`u4`/`u5`/`u6`/`c7` for
600 real frames. Pearson correlation across all `(this crate's field, chip's field)` pairs found
**no strong correspondence anywhere** -- the largest is a moderate 0.359 between this crate's own
`u_hat_4` and the chip's decoded `u4`, with everything else under 0.2 in magnitude (several near
zero). This is genuinely inconclusive rather than a clean negative: raw-integer Pearson correlation
is a weak tool for finding a *bit-level* correspondence that might be Gray-coded (as already
established for D-STAR/AMBE+2's own pitch field, §9) or reordered -- a real relationship would show
near-zero linear correlation under either scrambling even if the mapping is deterministic. The
`u_hat_4`/chip-`u4` pair is the one lead worth another session's time (e.g. checking a per-bit XOR/
Gray-decode correlation rather than raw integer correlation); nothing else in this table showed
enough signal to prioritize. The full 600-frame capture (every field from both sides, per frame) is
committed at `docs/references/ratet27_captures/u_vector_speech_correlation_600frames.tsv` so a
future session can pick up exactly where this one left off (re-analyze with a different
correlation technique) instead of re-running the same real-speech capture against the chip from
scratch.

**A third semantic-layer lead, prompted by re-reading this document's own earlier §9**: that section
found NOFEC mode's pitch parameter sits in the raw stream's third 12-bit slice (`u2` under NOFEC's own
contiguous convention), Gray-coded, with a distinctive "increases then saturates at a quantizer
ceiling" shape across frequency. Re-running that exact shape-check against FEC mode's own `g0`
(natural offsets 0-22, the *first* Golay block, decoded via `golay_decode` -- the block already
proven bit-for-bit identical to `fec.rs`) across the 16 frequencies already captured this session
(30-800 Hz, one converged frame each) finds a real, but weaker and messier, echo of the same shape:
Spearman rank correlation with frequency is a strong 0.897 across all 16 points, and `g0`'s decoded
value visibly separates into "generally lower, noisier" below ~200 Hz and "clustered near a ceiling
around 4040-4047" above it -- the same qualitative saturation signature as NOFEC's `u2`. **This is a
real lead, not a confirmed mapping**: restricted to frequencies inside AMBE's own documented 57-444
Hz voice-pitch range, the values are not cleanly monotonic (57->2853, 80->3045, 100->3041,
125->3938, 160->2217, 200->4042, 250->4040, 320->3946, 400->4041, 444->4043) -- a real dip at 160 Hz
and an early plateau by 200 Hz rather than a smooth curve -- so this could equally reflect several
parameters simultaneously destabilizing outside/at the edge of the vocoder's normal operating
assumptions (most of the tested frequencies are below or at the edge of the documented voice range)
rather than `g0` specifically carrying pitch. `g1` and `g2` show much weaker correlations (Spearman
-0.35 and -0.49) over the same data, for comparison. The full 16-frequency dataset and the analysis
script are committed at `docs/references/ratet27_captures/analyze_g0_pitch_hypothesis.py` (reads
`captured_frames_all5`-equivalent capture data) so a future session can re-test this specific
hypothesis with denser, purely in-voice-range frequency sampling rather than re-deriving it from
scratch.

**Follow-up, upgraded from lead to confirmed finding: `g0` genuinely carries a real pitch-related
quantizer in FEC mode.** The messy result above used single sine-tone captures at scattered
frequencies (including several outside AMBE's own voice-pitch range); a focused follow-up
(`examples/p25_ratet27_capture_dense_pitch_sweep.rs`) instead swept 20 frequencies *strictly within*
the documented 57-444Hz range at 20Hz steps, using the **sawtooth** signal (already established
elsewhere in this investigation as far more reliable than sine for convergence), with 60 settling
frames and 8 captured frames per frequency. Result: **19 of the 20 frequencies gave a perfectly
stable, single repeated `g0` value across all 8 captured frames** (only 80Hz showed the same kind of
low-frequency bistable oscillation already documented elsewhere in this investigation for D-STAR
and this crate's own encoder), and those 19 stable values form a clean, almost perfectly monotonic
decreasing staircase as frequency increases: `60->3945, 100->3065, 120->2745, 140->2237 (=160),
180->1085, 200->1597 (=220), 240->1149 (=260), 280->765 (=300=320), 340->445 (=360=380),
400->125 (=420), 440->61`. **Spearman rank correlation with frequency: -0.952.** The value
*decreasing* with increasing frequency matches the textbook `quantize_fundamental_frequency`
formula's own sign convention (`floor(4*pi/omega0_hat - 39)`, which decreases as frequency/omega0
increases), and the coarsening step size at high frequency versus finer steps at low frequency (a
real signature of any quantizer that's uniform in *period*, i.e. `1/frequency`, rather than in
frequency itself) matches the qualitative shape a genuine AMBE-family pitch quantizer should have.
One real anomaly, disclosed rather than smoothed over: 180Hz's value (1085) dips below both its
neighbors (140/160's 2237 and 200/220's 1597), breaking strict monotonicity at that single point --
plausibly a quantizer-boundary interaction with the discrete `L_hat`/`K_hat` harmonics-count step
function rather than a measurement error (every other point was perfectly frame-stable), but not
yet explained. **This does not need Gray-decoding to show the relationship** (unlike NOFEC mode's
own `u2` field, per §9) -- plain binary `g0` already correlates cleanly, a real, disclosed structural
difference between how the two modes' pitch-related fields are quantized/coded, not an
inconsistency in this investigation's own analysis. Taken together with `g0`'s already-confirmed
bit-for-bit identity to `fec.rs`'s Golay code, this is the strongest, most concrete semantic-layer
result of the session: **RATET(27) FEC mode's pitch-related parameter lives in `g0`** (wire
positions `{0,1,12,13,24,25,36,37,48,49,60,61,72,73,84,85,96,97,108,109,120,121,132}`), plain
binary, monotonically decreasing with frequency. The exact quantizer formula (bin edges, whether it
matches `quantize_fundamental_frequency`'s literal constants or a chip-specific variant) remains
unfit -- a natural next step given this clean staircase data is now committed and reproducible.

**Correction, found by the very next follow-up test: `g0` is not simply "the pitch parameter" --
it responds to amplitude too, most likely making it a gain/energy-related quantizer rather than a
pure pitch quantizer.** A clean amplitude sweep (`examples/p25_ratet27_capture_amplitude_sweep.rs`:
a single fixed, well-converged frequency (200Hz sawtooth) at 16 different, precisely-known peak
amplitudes -- unlike pseudo-random noise, whose actual RMS is decorrelated from its stated peak,
making the earlier gain-correlation attempt against noise data too noisy to be useful) found `g0`
correlates with amplitude just as cleanly as it correlated with frequency above: **Spearman 1.000**,
a perfect monotonic staircase from 1045 (quietest) to 1597 (loudest, saturating). This raised a real
concern that the frequency-correlation finding above might have been an amplitude/RMS confound
rather than a genuine pitch effect: a discretely-sampled sawtooth's *actual* RMS is not perfectly
frequency-independent when the period is a large fraction of the 160-sample frame (incomplete-cycle
boundary effects at low frequencies), even though its *nominal peak* was held fixed.

**Directly tested and ruled out as a confound, but the underlying dual-dependency is real.**
`examples/p25_ratet27_capture_rms_normalized_pitch_sweep.rs` reruns the exact same 57-444Hz dense
sweep, but explicitly computes each frequency's actual buffer RMS and rescales to a fixed target
(confirmed: 3463.4-3463.6 across all 20 frequencies, genuinely constant, not just nominally so).
**Every single decoded `g0` value came back bit-for-bit identical to the original, non-normalized
sweep** -- the exact same near-perfect monotonic staircase, Spearman -0.952. This rules out RMS
confound as the explanation for the frequency correlation: `g0` really does depend on frequency
independently of amplitude, *and* (per the amplitude sweep above) really does depend on amplitude
independently of frequency, at the same time. This is not a contradiction -- it's exactly the
behavior a genuine **gain/energy quantizer** should have in any real vocoder: gain is computed from
a spectral-amplitude decomposition that itself depends on where the harmonics fall relative to the
estimated pitch, so a real gain parameter is expected to shift with pitch even at constant overall
signal RMS, not just with amplitude. The likelier reading, revising the framing above rather than
retracting the underlying data (both correlations are real and reproduced identically across
independent captures): **`g0` is this chip's real gain/energy-related quantizer** (plausibly
`b_hat_2`/`g_hat[0]` in this crate's own textbook terms, the first-stage DC/gain DCT coefficient,
rather than the fundamental-frequency quantizer `b_hat_0`) -- still a genuine, useful semantic
result (a real parameter's real location, confirmed bit-exact and reproducible), just not the
specific parameter first guessed. Disentangling gain from pitch fully would need a 2-D sweep
(varying both independently and checking whether `g0` is better explained by a formula combining
both, e.g. log-energy at the fundamental) -- a concrete, bounded next step, with all three
datasets (dense pitch sweep, amplitude sweep, RMS-normalized sweep) committed for it.

**A small, independent corroboration of the gain interpretation, and a real negative result for
`g3`'s prediction-residual hypothesis.** Comparing already-captured silence frames against 200Hz
voiced frames across all 4 Golay blocks: `g0`'s silence value (1025) sits almost exactly at the
bottom of the range the amplitude sweep independently established (quietest tested amplitude gave
1045) -- a real, unforced consistency check supporting `g0` as a genuine gain/energy quantizer
(silence naturally reads as "near-minimum energy"). `g0`, `g1`, and `g3` all show *zero* overlap
between their silence-frame and voiced-frame value sets (consistent with several parameters all
being energy-sensitive, not necessarily each independently encoding "voicing" as a dedicated
decision); `g2` shows partial overlap. Separately, `g3`'s own rank-8 plateau was tested against one
more concrete hypothesis: since this crate's own encode pipeline includes a real frame-to-frame
*prediction residual* stage (differential encoding against previous-frame history,
`src/ambe/prediction.rs`), maybe `g3` carries part of a similar residual that only shows real
variation under large frame-to-frame discontinuities -- untested by this session's earlier
stimuli (steady tones, per-frame-independent noise, and *smoothly*-varying real speech).
`examples/p25_ratet27_capture_abrupt_transitions.rs` fed 2400 frames abruptly alternating between
maximally different states every single frame (loud-high-pitch / silence / loud-low-pitch /
quiet-high-pitch / noise / quiet-low-pitch, cycling). **Result: zero new distinct `g3` values
appeared** -- the same 149 distinct values already on record, even under the most aggressive
frame-to-frame discontinuity this investigation has tried. This rules out the prediction-residual
hypothesis specifically (or at least this particular way of trying to trigger it), leaving `g3`'s
real cause still open per the exhaustive-stimulus note above.

**D-STAR and AMBE+2 half-rate status, checked against this session's broader `/goal` directive, and
freshly re-run live against the real chip this session (not just cited from an earlier session's
claim)**: `examples/ambe_chip_validate_dstar.rs`, re-run live: **PASS, 40/40 frames at every one of
8 tested frequencies (50-1000Hz) Golay-decode with zero corrected errors on both `C0` and `C1`**.
`examples/ambe_chip_validate_ambe_plus_2.rs`, re-run live (the first attempt hit this investigation's
already-known transient chip `WouldBlock` timeout, discussed throughout this document; a clean
retry succeeded): **RATET(33) (half-rate with FEC): 10/10 zero-error frames under the "Annex H
deinterleaved" framing hypothesis** (the other hypothesis tested, direct `C0||C1||C2||C3`
concatenation, correctly gets 0/10 -- confirming this rate genuinely does use the textbook Annex H
interleave, unlike RATET(27)'s full-rate mode this session spent most of its time on).
RATET(34) (half-rate, No FEC): the sliding-window correlation scan reproduces its own
previously-established result (best candidate `bits[27..34)`, Gray-decoded, `|spearman|=0.964`).
Both harnesses are real, existing, chip-validated code from earlier sessions, and this session
freshly confirmed both still pass against the live chip today -- closing that part of the broader
goal with current, not merely historical, evidence.

**RATET(27) now has the same kind of real PASS/FAIL chip-validation harness D-STAR and AMBE+2 half-
rate already had.** `examples/ambe_chip_validate_ratet27.rs` -- new this session -- captures live
chip frames across the same 8 frequencies as the D-STAR harness and decodes each through
`ratet27_wire_format`/`ratet27_fec` directly (no search, no hypothesis-scoring -- this session
already determined the real format), checking for zero corrected errors on all 7 resolved blocks
(`g0`, `g1`, `g2`, `u4`, `u5`, `u6`, `c7`; `g3` deliberately excluded). **Live result: PASS, 120/120
captured frames across all 8 frequencies, zero errors on every block.** This supersedes the older,
now-stale `ambe_chip_validate_p25_wireformat.rs` (a search harness built on the wrong assumption
that RATET(27) uses PRN whitening, since disproven by this session's GF(2) rank analysis) and is
the concrete "duplicated in software, validated against the chip" deliverable for RATET(27)'s FEC
layer this whole session's work has been building toward.
