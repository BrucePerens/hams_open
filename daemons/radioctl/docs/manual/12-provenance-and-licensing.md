# 12. Provenance and licensing

radioctl is meant to be trusted with a transmitter, so it records where every fact about a radio came
from, and it is careful about what it may copy. This chapter is the project's practice, not legal advice.

## Licence

**AGPL-3.0-or-later** (Bruce Perens's decision of 2026-10-30, decision 1 of the proposal's section 11, taken
over the alternative of LGPL for easier linking by GPL tools). Every Rust file carries an
`SPDX-License-Identifier: AGPL-3.0-or-later` header and the manifests inherit the licence from the
workspace (`license = "AGPL-3.0-or-later"`). `crates/radioctl-core/tests/conventions.rs` fails if a file lacks
its header or a manifest does not inherit. `ham_digital_modes` in the same repository is LGPL, so mixed
licensing exists in `hams_open`; this crate does not link that code.

A program that links `radioctl-core` or talks to `radioctld` over the network is subject to the AGPL's
conditions where the licence applies. Which uses that covers is a question for the licence text and a
lawyer, not for this manual.

## Where the facts about radios come from

The order of preference, from the proposal's section 6:

1. **Vendors' published CAT and CI-V reference manuals.** Cited by document, revision, page and command, with
   the PDF's SHA-256 so a reader can confirm it is the same file. Facts about a protocol are not
   copyrightable; the manual's text is, so the files cite and do not copy.
2. **Captures of our own radios and owner-run tests.** None exist yet.
3. **Existing code in this repository** (the relay's CI-V and TS-2000 bridges, `tools/rig_simulator.py`),
   for the basic commands and for the simulators the virtual rigs replaced.
4. **Program output of Hamlib's `rigctl --dump-caps`** (Tier 1). Program output, not source. It feeds
   the capability matrix ([appendix A](A-capability-matrix.md)) and some descriptions' timing.
5. **Hamlib's source, read for facts only**, and only where no usable vendor manual exists. This is the
   precedent of the `rsp_open` driver ("facts only, no code copied"). Each fact has `kind = "hamlib-source"` with
   the Hamlib release, the file and the function, and a note on why a manual was not the source. No Hamlib
   code or table is copied; layouts and numbers are restated as facts. The proposal also describes a stricter
   two-person separation (one reads, another writes) as available if wanted; the decision of 2026-10-30 was
   "facts with provenance". Hamlib is LGPL.

**The clean-room rules in practice:**

- Do not paste a Hamlib function, table or comment, and do not transliterate one. Write the engine from the
  facts, in this project's own structure.
- Every fact that came from Hamlib names its file and function so a reviewer can check it, and says so in
  the description file. A table of numbers is restated only where it is a fact about the radio (a command
  number, a payload layout), not Hamlib's arrangement of it.
- Hamlib is also an **oracle**: its `rigctld` and `rigctl` binaries are run in tests and compared with.
  Running a program and comparing outputs copies nothing.
- A document whose terms forbid reuse is **not used**, and the fact that it was skipped is recorded (below).
  The project's standing rule is to skip such sources rather than ask for permission.

## What is skipped, and why

| Source | Decision | Reason |
|---|---|---|
| Icom CI-V reference guides (IC-7300, IC-705, IC-7610, IC-9700) | **Not used.** | The download service of icomjapan.com is licensed to individual use and forbids reuse of the manuals' data (no commercial use, no transfer of data). The four Icom descriptions and the CI-V engine's extra commands and the scope support were read from Hamlib 4.6.2 source for facts only, with file and function per fact. |
| "CI-V implementation of Xiegu X6200" (an extract hosted by Radioddity, firmware V1.0.6) | **Not used.** | It says it may not be "reproduced or used in any way" without written permission. The Xiegu descriptions are deliberately small and use Hamlib facts only; the X5105 and X6200 descriptions are Hamlib facts only as well. |
| "X5105 Instruction Manual" (Xiegu, V1.0.04-02, xiegu.eu) | **Read, not used.** | Its copyright notice reserves all rights and forbids copying any part without permission, and its computer-control section says only that a standard CIV instruction set is used (no address, command or mode table). Nothing was taken from it. |
| Kenwood TS-2000 command guide | Read from a third-party transcription (manualsdir.com, with the text in reverse character order). | Kenwood hosts no TS-2000 command guide file. The page numbers in the facts come from that transcription, and the facts say it is itself unverified. |
| FlexRadio "messages.h" (embedded in the SmartSDR API wiki page "Known API Responses") | **Not used.** | Its own comment prohibits unauthorised use, duplication or distribution. Only the response codes printed in the per-command tables of the wiki pages that were used appear in the engine, and they are only logged. |
| FlexRadio SmartSDR API wiki (github.com/flexradio/smartsdr-api-docs) | Used for facts only. | The repository carries no licence or terms file and no page states terms for copying; command spellings, field names and value ranges are used, no text. The commit is named in the engine. |
| OpenHPSDR protocol documents (Protocol 1 and 2) | Used for facts. | The `hpsdr` crate's README (`daemons/hpsdr/README.md`, "Sources and provenance") names the documents and says no terms forbid reuse of the facts and no source code of other OpenHPSDR programs was read. |
| Yaesu operating manuals (FT-817ND, FT-857D, FT-897D) for the legacy family | Used. | The CAT chapters; the opcode chart's notes are figures, read from rendered pages. The one fact they omit (the PTT bit's level) is from Hamlib and labelled so. |
| Kenwood TS-590S/SG PC command reference; Yaesu CAT Operation Reference Manuals | Used. | The engine comments record no reuse restriction beyond the usual disclaimer or copyright notice. |
| Elecraft K3S/K3/KX3/KX2 Programmer's Reference, Rev. G5 | Used. | A manual Elecraft publishes for download; the repository does not record its terms of use beyond that, so re-check them before reusing more than the cited facts. |

## Citing documents in this repository

- Link to a vendor document or name it with its revision and SHA-256; never paste its text or its tables.
  The Elecraft Programmer's Reference Rev. G5 (February 20, 2019) is at
  `https://ftp.elecraft.com/KX3/Manuals%20Downloads/K3S&K3&KX3&KX2%20Pgmrs%20Ref,%20G5.pdf`, SHA-256
  `015b60c1b5f44346247a00375a23174012ce4da66a6a27eaa00c00c20877f95d`. The other documents are named in
  the engine module comments and in each fact, with file names and hashes.
- Quote at most a short phrase, with attribution. Summarise otherwise.
- Do not put serial numbers, addresses, keys or captured personal data in descriptions, transcripts or
  tests.
- Downloaded manuals are untrusted data: keep them outside the repository, and pin their hash in the fact.
- Say what was checked. A fact is `unverified` until someone compares it with a document
  (`manual-checked`), replays it against a transcript (`transcript-checked`) or runs it on a radio
  (`hardware-verified`). Chapter [6](06-rig-descriptions.md) has the states.

## Hamlib issue 138

The design lessons drawn from Hamlib issue 138 (<https://github.com/Hamlib/Hamlib/issues/138>) are the
proposal's own reading of a public bug report; chapter [4](04-api.md) summarises them in this project's
words. No Hamlib text is reproduced.
