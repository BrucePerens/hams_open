# radioctl

A thread-safe, async-only Rust rig-control library intended to replace Hamlib in the relay, plus a
`rigctld`-protocol server (`radioctld`).

- Licence: AGPL-3.0-or-later (Bruce's decision of 2026-10-30). Every Rust file carries an SPDX header
  and the manifests inherit the licence from the workspace.
- Planning: `hams_com/docs/proposals/RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`.
- Programmer's manual: [`docs/manual/README.md`](docs/manual/README.md).
- Design decisions: [`docs/DESIGN.md`](docs/DESIGN.md).

## Status

- Phase 0/1 foundation. Nothing here has been used with real hardware; treat every item below as
  `unverified` unless it says otherwise.
- Implemented (tests on `main`, no radio):
  - Vocabulary, sans-IO protocol engines and virtual rigs.
  - Rig actor with queue, cache, polling, events and the transmit gate.
  - Real serial (USB-serial identified by vid/pid/serial, hot-plug reconnect, gated line keying) and TCP/UDP network links (reconnect with backoff).
  - `radioctld` (rig and rotator servers), the protocol splitter, `radioctl-station` and a C ABI sketch.
- Protocol families: Icom CI-V, Kenwood ASCII (including the TS-890S dialect), Elecraft KX3, Yaesu newcat and legacy five-byte, FlexRadio SmartSDR, OpenHPSDR (control plane only), Xiegu (CI-V).
- Rig descriptions: 126 files in `rigs/` (list below). All facts are `unverified` until a volunteer confirms them with `docs/VOLUNTEER_TESTING.md`; no description is `hardware-verified`.
- Transmit: no transmit path to real hardware has been run. Anything that can transmit must fail closed behind an explicit capability gate.

## Layout

```
daemons/radioctl/
  Cargo.toml              workspace (resolver 2), shared lints: no unsafe, no unwrap/expect/panic
  rust-toolchain.toml     pinned toolchain
  radioctld.example.toml  annotated example configuration
  crates/radioctl-core/       typed values, capability model, errors, rig actor, network links, rig-file loader
  crates/radioctl-protocols/  sans-IO protocol engines (one per family), virtual rigs, conformance replay, hpsdr bridge
  crates/radioctl-splitter/   virtual IC-7300 CI-V / TS-2000 CAT endpoints (pty, TCP) in front of one rig actor
  crates/radioctld/           rigctld text-protocol server (library + binary `radioctld`) and the examples
  crates/radioctl-station/    station control: switches, power, interlocks, radio manager, Home Assistant, rotators, amplifier/tuner channels (own README)
  crates/radioctl-capi/       C ABI sketch (cdylib, staticlib, cbindgen header include/radioctl.h; own README)
  rigs/                   rig description files (*.toml) and rigs/transcripts/
  docs/DESIGN.md          design note (ADR style)
  docs/manual/            programmer's manual, one file per chapter (start at docs/manual/README.md)
  tools/hamlib_caps_matrix.py   Tier 1 generator (rigctl --dump-caps -> matrix)
  tools/rig_support_matrix.py   writes docs/RIG_SUPPORT.md (Hamlib models vs. descriptions vs. verification)
  data/hamlib-<version>/  generated capability_matrix.json and HAMLIB_DIFFERENCES.md
```

- The manual's code blocks are checked by `crates/radioctld/tests/manual.rs`.
- The manual's runnable examples are in `crates/radioctld/examples/` (see "Examples" below).

### Layout changes from the proposal

Record every change here.

- **Crate names and dependency direction**
  - The crates are used as named.
  - `radioctl-protocols` depends on `radioctl-core`.
  - `radioctl-core` holds the vocabulary: `Hz`, `Mode`, `Vfo`, `Op`, `Observation`, `Capability`, `RigDescription`, `RigError` and the `ProtocolEngine` trait.
  - `radioctld` depends on both.
  - The actor in `radioctl-core` is generic over `ProtocolEngine`, so core never depends on the protocols crate.
- **`RigDescription`**
  - It is a Rust struct.
  - The two built-in descriptions (IC-7300 style CI-V, TS-2000 style Kenwood) are written in code from our own simulator and bridges and are unverified.
  - The TOML format (`rigs/*.toml`, below) deserialises into the same struct.
- **Virtual rigs**
  - They live in `radioctl-protocols::virtual_rig` and are sans-IO like the engines.
  - The optional `io` module (cargo feature `tokio-io`, default on) runs one behind an in-memory duplex stream with a reply delay and a control handle.
  - `radioctl-core` dev-depends on `radioctl-protocols` for its actor tests.
- **`radioctl-splitter`**
  - It is a separate crate so the library crates stay free of pty code.
  - It depends on core and protocols.
  - `radioctld` uses its PTT arbiter (`PttOwner`), so the two share one set of ownership semantics.
- **`radioctl-station` and `radioctl-capi`**
  - Both are workspace crates; the station layer is a radioctl module (Bruce, 2026-10-30), not a sibling crate.
  - `radioctld` serves rotators through the station crate's engines.
- **Cargo feature `owned-runtime`** (`radioctl-core`, off by default)
  - It is the one place the library builds a tokio runtime of its own, for programs that are not tokio programs.
  - `radioctl-capi` enables it by default for C callers.
  - `radioctld` does not use it: the binary builds the one multi-thread runtime of its process and passes its `Handle` to the library, which creates none.
- **Dependency policy**
  - This workspace has no `deny.toml`.
  - Only `daemons/relay_broker/deny.toml` exists in the repository; add one here when the repository gets a shared policy.

## Build and test

Run from `daemons/radioctl`, with a target directory of your own that you delete when the job is done. Never use a per-worktree `target/` (AGENTS.md).

```
export CARGO_TARGET_DIR=$HOME/.cache/hams-cargo-target-radioctl
nice -n 10 cargo fmt --check
nice -n 10 cargo clippy --all-targets --all-features -- -D warnings
nice -n 10 cargo test --all-features --no-fail-fast
```

- `--all-features` turns on the `hamlib-oracle` tests. They need Hamlib's `rigctld` and `rigctl` binaries (or `RIGCTLD=/path`, `RIGCTL=/path`) and fail loudly without them.
- Without the feature the rest runs on a machine with no Hamlib.
- Toolchain: pinned in `rust-toolchain.toml`.
- Test layers and what each proves: manual chapter 11 ([`docs/manual/11-testing.md`](docs/manual/11-testing.md)).

### Differential tests against Hamlib

```
nice -n 10 cargo test -p radioctl-protocols --features hamlib-oracle --test rigctld_oracle -- --nocapture
nice -n 10 cargo test -p radioctld --features hamlib-oracle
```

- Each rig's virtual twin is driven twice: once through Hamlib's `rigctld` (started on a pty whose master is the virtual rig) and once through radioctl's engine.
- The readings and final states are compared, and the bytes Hamlib sent are printed.
- The harness decides that `rigctld` is ready from the kernel's tables (`/proc/net/tcp`, `/proc/<pid>/fd`), not by connecting, and turns Hamlib's cache off so every answer is read from the radio. It retries when `rigctld` lost the port race and reaps the processes it started.
- `radioctld`'s own oracle runs Hamlib's `rigctl` client against a virtual KX3, IC-7300 and TS-590SG, and `rotctl -m 2` against the virtual rotator.
- First results (Hamlib 4.6.2):
  - KX3 and IC-7300 agree on frequency, mode, PTT read, RIT offset and keyer speed.
  - The TS-2000 differs because Hamlib's backend reads the frequency from `IF;`, which the virtual TS-2000 refuses (layout unverified). A test pins that known gap.
  - The oracle found and fixed two virtual-rig errors: IC-7300 `26 00` lacked the DATA flag byte, and the virtual KX3 lacked `PS`, `K2`/`K3`, `OM`, `IF`, `RV`, which Hamlib's KX3 backend sends at start-up.
- Known disagreements are pinned in `tests/rigctld_oracle.rs`: `known_icom_divergences`, `known_ts890_divergences`.

### Other test layers

- Property tests (`proptest`): `radioctl-protocols/tests/robustness.rs` (Yaesu, Flex, scope, HPSDR engines and `hpsdr` packet parsers; a virtual-rig panic found this way was fixed) and `radioctld/tests/parser_props.rs`.
- Hand-written transcripts for the FLEX-6400 and ANAN-100 are replayed through a per-family stand-in.
- Convention test: `crates/radioctl-core/tests/conventions.rs` (see "Conventions").
- Manual test: `crates/radioctld/tests/manual.rs`.
- Virtual-rig fault injection: jitter, echo, chunked and truncated replies, dropped replies (manual chapter 11).

## Rig descriptions and conformance

### Files and format

- `rigs/*.toml` are rig description files (schema 1; format documented at the top of `crates/radioctl-core/src/rigfile.rs`).
- Every datum must be explained by a `[[facts]]` entry with:
  - a `provenance`: manual, capture, owner, repo-code, program-output (also `hamlib-source`, see below);
  - a `verified` state: `unverified`, `manual-checked`, `transcript-checked`, `hardware-verified` (with radio, firmware, date and what was tested).
- A file with an unexplained or unknown datum does not load.
- Optional `[levels]` block: `levels.attenuator_db` (receive attenuator steps) and `levels.preamp_steps` (preamplifier steps). `Level::Attenuator` takes dB; `Level::Preamp` takes a step number.
- Optional `[scope]` block (`scope.spans_hz`, icom-civ only).
- `rigs/transcripts/*.toml` are wire transcripts replayed by `radioctl_protocols::conformance::replay` against the engine and the virtual rig generated from the same description (tests in `crates/radioctl-protocols/tests/rig_files.rs`).
- Everything in these files is unverified, and the transcripts are hand-written from our own simulator and bridge code (each `origin` says so). They prove that the description, the engine and the virtual rig agree with each other, not that they agree with a radio.
- A real capture or an owner's bench run is what upgrades a `verified` field.

### Rig list

| Family | Files in `rigs/` | Source of the facts |
|---|---|---|
| Elecraft | `elecraft-kx3`, `elecraft-k2`, `elecraft-k3`, `elecraft-k3s`, `elecraft-kx2`, `elecraft-k4` | Elecraft Programmer's Reference Rev. G5, cited by command heading and page, with the PDF's sha256; transcript hand-written from the same manual |
| Icom CI-V | `icom-ic-7300`, `icom-ic-705`, `icom-ic-7610`, `icom-ic-9700`, `icom-ic-7000`, `icom-ic-7100`, `icom-ic-7200`, `icom-ic-7410`, `icom-ic-7600`, `icom-ic-7700`, `icom-ic-7760`, `icom-ic-7800`, `icom-ic-7851`, `icom-ic-9100`, `icom-ic-r8600`, `icom-ic-r75`, `icom-ic-r9500`, `icom-ic-706`, `icom-ic-706mkii`, `icom-ic-706mkiig`, `icom-ic-718`, `icom-ic-746`, `icom-ic-746pro`, `icom-ic-703`, `icom-ic-756`, `icom-ic-756pro`, `icom-ic-756proii`, `icom-ic-756proiii`, `icom-ic-910`, `icom-ic-2730`, `icom-ic-78`, `icom-ic-707`, `icom-ic-729`, `icom-ic-736`, `icom-ic-737`, `icom-ic-738`, `icom-ic-775`, `icom-ic-781`, `icom-ic-970`, `icom-ic-735`, `icom-ic-905`, `icom-ic-725`, `icom-ic-726`, `icom-ic-728`, `icom-ic-751`, `icom-ic-761`, `icom-ic-765`, `icom-ic-1275`, `icom-ic-271`, `icom-ic-275`, `icom-ic-375`, `icom-ic-471`, `icom-ic-475`, `icom-ic-575`, `icom-ic-r71`, `icom-ic-r72`, `icom-ic-r7000`, `icom-ic-r7100`, `icom-ic-r8500`, `icom-ic-r9000`, `icom-ic-rx7`, `icom-id-5100`, `icom-id-1`, `icom-id-31`, `icom-id-4100`, `icom-id-51`, `icom-ic-r6`, `icom-ic-r10`, `icom-ic-r20`, `icom-ic-r30`, `icom-ic-820h`, `icom-ic-821h`, `icom-ic-92d`, `icom-ic-f8101` | Hamlib 4.6.2 source (facts only) and `rigctl --dump-caps` output (see below); for the 19 later models, Icom's instruction manuals (archive.org copies) where one could be read, facts only, `manual-checked` per fact |
| Kenwood | `kenwood-ts-2000`, `kenwood-r-5000`, `kenwood-trc-80`, `kenwood-ts-930`, `kenwood-ts-590s`, `kenwood-ts-590sg`, `kenwood-ts-890s`, `kenwood-ts-50s`, `kenwood-ts-140s`, `kenwood-ts-440s`, `kenwood-ts-450s`, `kenwood-ts-480`, `kenwood-ts-570d`, `kenwood-ts-570s`, `kenwood-ts-680s`, `kenwood-ts-690s`, `kenwood-ts-711`, `kenwood-ts-790`, `kenwood-ts-811`, `kenwood-ts-850`, `kenwood-ts-870s`, `kenwood-ts-990s`, `kenwood-ts-940s`, `kenwood-ts-950s`, `kenwood-ts-950sdx` | Kenwood's published PC command references (document, page and, for the TS-590 guide, the PDF's sha256 per fact); the sixteen newer files from Hamlib 4.6.2 facts only |
| Kenwood handhelds and mobiles | `kenwood-th-d7a`, `kenwood-th-f6a`, `kenwood-th-f7e`, `kenwood-th-g71`, `kenwood-tm-d700`, `kenwood-tm-v7`, `kenwood-th-d72a`, `kenwood-tm-d710`, `kenwood-tm-v71`, `kenwood-th-d74` | Hamlib 4.6.2 facts only; every fact unverified |
| Xiegu (CI-V) | `xiegu-g90`, `xiegu-x6100`, `xiegu-x6200`, `xiegu-x108g`, `xiegu-x5105` | Hamlib facts only, with Hamlib's shared addresses |
| Yaesu newcat | `yaesu-ft-991a`, `yaesu-ftdx-9000-old`, `yaesu-ftdx10`, `yaesu-ftdx101d`, `yaesu-ft-450d`, `yaesu-ft-450`, `yaesu-ft-950`, `yaesu-ft-2000`, `yaesu-ftdx-1200`, `yaesu-ftdx-3000`, `yaesu-ftdx-5000`, `yaesu-ftdx-9000`, `yaesu-ft-891`, `yaesu-ft-710`, `yaesu-ftdx-101mp` | Yaesu's CAT Operation Reference Manuals (document, PDF page and sha256 per fact); the FT-450 from Hamlib 4.6.2 facts only |
| Yaesu legacy five-byte | `yaesu-ft-817`, `yaesu-ft-857d`, `yaesu-ft-897d`, `yaesu-ft-897`, `yaesu-ft-847`, `yaesu-ft-847uni`, `yaesu-ft-650`, `yaesu-ft-736r`, `yaesu-ft-818` | Operating manuals' CAT chapters The FT-897, FT-847, FT-847UNI, FT-650 and FT-736R files are from Hamlib 4.6.2 facts only (the FT-897 shares the FT-897D's CAT chapter); the last three are set-only. |
| AOR receivers (ASCII `RF`/`MD`/`LM`) | `aor-ar8000`, `aor-ar8200`, `aor-ar8600`, `aor-ar5000`, `aor-ar5000a`, `aor-ar2700`, `aor-sr2200` | Hamlib 4.6.2 source (facts only) |
| Uniden scanners (ASCII `RF`/`RM`/`SG`) | `uniden-bc245xlt`, `uniden-bc250d`, `uniden-bc780xlt`, `uniden-bc895xlt`, `uniden-bc898t` | Hamlib 4.6.2 source (facts only) |
| JRC (ASCII `F`/`D`/`I`) | `jrc-nrd-525`, `jrc-nrd-535d`, `jrc-nrd-545`, `jrc-jst-145`, `jrc-jst-245` | Hamlib 4.6.2 source (facts only) |
| Icom PCR (ASCII `K0`) | `icom-ic-pcr100`, `icom-ic-pcr1000`, `icom-ic-pcr1500`, `icom-ic-pcr2500` | Hamlib 4.6.2 source (facts only) |
| Icom marine (`$PICOA`) | `icom-ic-m700pro`, `icom-ic-m710`, `icom-ic-m802`, `icom-ic-m803` | Hamlib 4.6.2 source (facts only) |
| Rohde and Schwarz (ASCII `FREQ`/`DEM`) | `rs-esmc`, `rs-eb200` | Hamlib 4.6.2 source (facts only) |
| OptoScan boards (CI-V at 0x80) | `optoscan-os535`, `optoscan-os456` | Hamlib 4.6.2 source (facts only) |
| Yaesu legacy five-byte | `yaesu-ft-817`, `yaesu-ft-857d`, `yaesu-ft-897d`, `yaesu-ft-818` | Operating manuals' CAT chapters |
| FlexRadio | `flex-6300`, `flex-6400`, `flex-6600`, `flex-6700` | FlexRadio's SmartSDR API wiki |
| OpenHPSDR | `anan-10`, `anan-100` | `daemons/hpsdr` and the OpenHPSDR protocols |

### Icom CI-V

- The first four Icom files (IC-7300, 705, 7610, 9700) are **not written from Icom's guides**. For the 19 later models Icom's instruction manuals were read for facts, under Bruce Perens' decision of 2026-10-31 (the terms are unreasonable and would not survive a court): Icom's download endpoint is excluded by robots.txt and the direct path is 403, so the archive.org mirror copies were read; they are kept outside every repository (`~/manuals/icom/`, sha256 in `SHA256SUMS`) and are not redistributed.
- Their facts were read from Hamlib 4.6.2 source (facts only, provenance kind `hamlib-source`, file and function named per fact) and from `rigctl --dump-caps` output.
- Engine features: VFO B, split, RIT/XIT, levels, filter, model id.
- Raw meters (`Op::GetMeter`: SWR, ALC, COMP, Vd, Id as 0 to 255 deflections, uncalibrated) and data modes (`USB-D`, `LSB-D`; `M PKTUSB` / `PKTLSB` through `radioctld`) are implemented for the IC-7300. The engine takes the mode being set as the mode for the filter table.
- Scope (command 0x27) is in `crates/radioctl-protocols/src/civ_scope.rs`:
  - reachable through `Op` (`SetScopeOutput`, `SetScopeAxis`, `SetScopeSpan`, `SetScopeEdges`, `SetScopeReference` and their reads), capability `scope`, on the IC-7300 and IC-7610;
  - `CivEngine` returns each complete sweep as `Inbound::ScopeFrame`, a typed `radioctl_core::scope::ScopeFrame`;
  - the actor publishes sweeps on `RigHandle::scope_frames()`, a stream apart from the status-change events.
- Batch of October 31, 2026 (19 more models, same Hamlib facts, none verified):
  - IC-7000, 7100, 7200, 7410, 7600, 7700, 7760, 7800, 7850/7851, 9100, R8600, R75, R9500, 706, 706MkII, 706MkIIG, 718, 746 and 746PRO.
  - Each file has the CI-V address, the modes Hamlib lists, the attenuator and preamplifier steps and the capabilities Hamlib's list for the model has. The description format has no field for frequency ranges, filter tables or function lists, so they are not repeated in the files.
  - A command the model lacks in Hamlib is left out, not guessed: the receivers (R8600, R75, R9500) have no PTT or split; the IC-706, 718 and 756 (first versions) have no PTT command; the second VFO is claimed only where Hamlib says command 25 is possible (IC-7100 and the Main/Sub radios).
  - Data modes (command 26) only where Hamlib treats 26 as always present (IC-7760 and IC-7850/7851); the others use forms (`1A 06`) the engine does not send.
  - Hamlib's Main/Sub flow (`07 C2`, `07 D0`) differs from radioctl's on the IC-7600, 7800, 7850/7851 and 7760, and the IC-7700 ends in a different mode: pinned as open questions in `known_icom_batch_divergences`.
  - Icom's manuals were read for the IC-7200, 7410, 7600, 7700, 7800, 7850/7851, 9100, R75, R9500, 746, 746PRO, 706MkIIG (CI-V command tables) and the IC-7100, R8600, 718 (CI-V address only). Not obtainable as readable text: the IC-7000 (only a service manual), IC-7760, IC-706 and IC-706MkII (image scans without text). Each fact states which: `manual-checked` where the page was read (CI-V address, mode codes of the IC-7200, 7410 and 9100, split `0F`), otherwise still Hamlib's.
  - Corrections from the manuals: the IC-7600, 7700 and 7800 tables have no command 25 and no command 21, so `second-vfo` and `rit-xit` were removed from those three files. The IC-706MkIIG table has no command 14: fixed by `levels.absent`, a per-description list of the plain `14` levels a model lacks (AF, RF, mic, monitor, keyer, power), which the engine and the virtual rig then refuse.
  - Third batch (16 models, plus the IC-735 and IC-905 for the new `civ.frequency_bytes` field: 4 bytes for the IC-735, 5 or 6 for the IC-905; the IC-820H and 821H wait for their unit, Hamlib facts only, no manual read, none verified): IC-703, 756, 756PRO, 756PROII, 756PROIII, 910, 2730, 78, 707, 729, 736, 737, 738, 775, 781 and 970. `levels.absent` is set from the levels Hamlib's list for the model lacks. On the IC-756 family and IC-2730 Hamlib's split starts with `07 D0` (Main band), which the virtual rig does not model: pinned as open questions like the IC-7600's.
  - Fourth batch (20 models, Hamlib facts only, no manual read, none verified): IC-725, 726, 728, 751, 761, 765, 1275, 271, 275, 375, 471, 475, 575 and the receivers R71, R72, R7000, R7100, R8500, R9000, RX7.
  - Fifth batch (9 models, Hamlib facts only, no manual read, none verified): the D-STAR models ID-5100, ID-31, ID-4100 and ID-51 (new mode `d-star`, CI-V mode 17), ID-1 and the handheld receivers IC-R6, R10, R20, R30. Hamlib modes without a radioctl name (FMN, AMN, SAM, P25, DPMR, NXDN, DCR) are listed in each file as not described. The IC-92D (Hamlib has no frequency or mode functions for it) and IC-F8101 (frequency and mode through `1A 34`/`35`/`36`) are not described yet, see the plan.
  - Still to do: the list in `docs/RIG_COVERAGE_PLAN.md`.
- `baud = "auto"` (or `baud_probe = [...]`) in `radioctld` runs a read-only frequency read at several baud rates (off unless configured; manual chapter 5).

### Yaesu newcat and Elecraft (batch of October 31, 2026)

- 11 Yaesu newcat models and 4 Elecraft models, all `unverified`. Sources: Yaesu's CAT Operation Reference Books for the FT-450D, FT-950, FT-2000, FTDX1200, FTDX3000, FTDX5000, FTDX9000 (FT DX 9000 Series), FT-891, FT-710 and FTDX101MP/D (each fact names the file, the PDF page and the sha256); Elecraft's K3S/K3/KX3/KX2 Programmer's Reference Rev. G5 (K3, K3S, KX2) and the K4 Programmer's Reference Rev. D11. The FT-450 uses Hamlib 4.6.2 facts only (`hamlib-source`), because Yaesu's FT-450D guide is for another radio.
- The FT-450, FT-450D, FT-950, FT-2000, FTDX1200, FTDX3000, FTDX5000 and FTDX9000 write the frequency with **8 digits** (the IF answer is 27 characters): `yaesu.frequency_digits = 8` in the description, implemented in the engine and in the virtual rig; the others use 9.
- A command the guide has in a different form is left out, not guessed: no `levels` on the FT-450D, FT-950, FT-2000, FTDX5000 and FTDX9000 (MG and PC are 0-255 there, the engine's ranges are the FT-991A's 0-100 and 5-100) or on the FT-891 (RG is 0-30); no `split` on the dual-receiver models (FT selects the main or sub band transmitter) or the FT-891 (ST); no `rit-xit` on the FT-450 family, FT-891 and FT-710; `read-power` only where RM 5 is the PO meter.
- The K4 is described in its K3-compatible mode only: frequency, modes, PTT (TQ, TX, RX), KY text and AI; its AG, RG, KS, MG, PC, RA and PA differ from the K3 forms the engine sends.
- The FT-450 was compared with its CAT Operation Reference Book (public archive.org copy, SHA-256 in each fact): ID 0241, the 8-digit frequency field, the 27-character IF answer and the mode letters agree and are `manual-checked`; `read-power` (RM5, the PO meter) is new. Levels, RIT/XIT and split are still not claimed (MG is three sub-ranges of 000-255 and PC is 000-255 in the book; no XT command; FT selects the displayed or opposite band).
- Hamlib agrees with the engine on 12 of the 15 on every step the oracle harness can express; the FT-450, FT-450D and FTDX-9000 differ on the second-VFO steps (`FT1;`), pinned in `known_ascii_batch_divergences`. Two ID disagreements are recorded in the files: the FT-891 (guide 0650, Hamlib 135) and the FTDX-3000 (guide 0462, Hamlib 460).
- Not done: the K2 (needs a K2 dialect of `IF`/`MD`), XG3 (a signal source, not a transceiver), FTDX-9000 "Old" (Hamlib's separate model; its ID is not in the guide). See `docs/RIG_COVERAGE_PLAN.md`.

### Kenwood

- Batch of October 31, 2026 (16 more models: TS-50S, 140S, 450S, 480, 570D, 570S, 680S, 690S, 711, 790, 811, 850, 870S, 940S, 950S and 950SDX, none verified):
  - **Manuals, second pass (October 31, 2026).** The first pass used Hamlib 4.6.2 facts only, because kenwood.com refused automated downloads (an "Access Denied" page, robots.txt included) and third-party copies restrict redistribution. Bruce decided that redistribution-restricted manuals may be downloaded and read for facts, never redistributed. kenwood.com is still not fetched (the block is not worked around); the copies came from archive.org, a public mirror (people.freebsd.org) and qsl.net, kept outside every repository under `~/manuals/` with their SHA-256 in each fact. The TS-480 (PC control command reference, 2003/11/28) and TS-870S (instruction manual, appendix D) were read and their command facts are `manual-checked`; the FT-450 is listed under Yaesu. The other fourteen models' manuals were downloaded, but those that hold a command table are scanned images that have not been read, so their facts stay `hamlib-source` and `unverified`.
  - Found by the second pass: the TS-480 guide gives RG as 000-100 (the engine writes up to 255; pinned in `ascii_families.rs`); the TS-870S has parameterless RU/RD, `AGnnn`, `SMnnnn`, `KS` 001-100 and `PC` 010-100, so its description no longer claims `rit-xit` (levels and signal were never claimed).
  - Engine: the classic Kenwood engine (35-character `IF` body, `MD` letters, `FA`/`FB` of 11 digits), which is also what Hamlib's generic backend assumes for these models. No engine change was needed.
  - A command the model lacks in Hamlib is left out, not guessed: `read-ptt`, `split`, `rit-xit`, `transceive` only where Hamlib has the matching function; `second-vfo` only where Hamlib can set VFO B (not on the TS-140S, 680S, 711, 811, 950S and 950SDX); `levels` and `read-signal` only on the TS-480 (whose guide has now been read); the AG/RG/PC/KS forms of the older models differ and have not been read yet.
  - The TS-680S has no `identify`: Hamlib supposes it answers `ID006;` like the TS-140S and cannot tell them apart. The TS-940S has no `rtty` mode: Hamlib's mode table has no code for it.
  - Hamlib reads the second frequency back with `FA;` where radioctl reads `FB;` on the TS-450S, 480, 570D and 570S: pinned in `known_ascii_batch_divergences`. The other six agree on every step.
  - Older dialect (`kenwood.dialect = "ts440"`): the TS-440S, from the IC-10 manual's command pages (read as images; `manual-checked` for the pages named in its facts): `MD` only sets, the mode and transmit VFO are read from the 38-character `IF`, split is `FN0;SP<n>;`, `RU`/`RD` take no parameter (an exact RIT offset is refused), and there is no `SM` or level command. Tests: `tests/ts440.rs`.
  - Checked against the manuals (2026-10-31): the TS-570S/D appendix and the TS-450S/TS-690S external-control manual (tables read as page images). TS-570: `RU`/`RD` take no parameter (so `rit-xit` was removed, as for the TS-870S), `AG` is `AGnnn` and `SM` is `SMnnnn` (so no levels or signal), modes 7 and 9 added. TS-450S/TS-690S now use `kenwood.dialect = "ts450"` (`MD` and `FR`/`FT` set only, mode and transmit VFO read from `IF`, `AI1`). Tests: `tests/ts440.rs`.
  - Nine older models (TS-50S, 140S, 680S, 711, 790, 811, 850, 940S, 950S/SDX): their instruction manuals (text layer, or OCR for the TS-950SDX scan) carry no PC command tables, only the optional IF-232C interface; each description's capability fact says which manual was read. They stay `unverified`.
  - TS-990S (`kenwood.dialect = "ts990"`, from Kenwood's PC command reference, `manual-checked` per page): the main band only (`FA`), the mode as `OM0`, no split, PTT read or levels (band digits and other ranges), `RIT`/`XIT` and `SM0` as the classic set. Its LAN stream (`##CN`, `##ID`) is not implemented and unverified. Tests: `tests/ts990.rs`.
  - Not done: the R-5000 (Hamlib's IC-10 dialect: 32-character `IF`), the TS-930 (no known `ID`), the TS-990S sub receiver, levels and LAN. The TH and TM handhelds are a different command family. See `docs/RIG_COVERAGE_PLAN.md`.
- The TS-2000 text was read from a third-party transcription of its manual, which the facts say.
- The `IF;` layout that was unverified is resolved in `kenwood.rs`.
- The **TS-890S** (`kenwood-ts-890s`, dialect `ts890`) has its own engine, `kenwood_ts890.rs`, written from its own guide (January 30, 2019):
  - no `IF`; `OM` carries the mode; split goes through `TB`;
  - the LAN session `kenwood_ts890_lan` (`##CN;`, `##ID` login, keep-alive) is wired into `radioctld`'s `tcp` transport as `framing = "ts890-lan"`;
  - tested against a fake radio on loopback only;
  - the bandscope is not implemented;
  - Hamlib's TS-890S backend reads with `IF;`, which the guide does not list, so the oracle records the frequency and mode steps as divergences (`known_ts890_divergences`).

### Xiegu

- The Xiegu files are deliberately small and use Hamlib facts only.
- The one public Xiegu CI-V document found (Radioddity's X6200 extract) forbids use without written permission, so it was skipped.
- The X5105 Instruction Manual's copyright-reserved text was skipped too (it holds no CI-V table anyway).
- The X5105 and X6200 descriptions use Hamlib's shared addresses.

### Yaesu

- Newcat files use the family `yaesu-newcat` with a `[yaesu]` block.
- The FTDX101D copy of the manual is a mirror of the yaesu.com PDF.
- Split is claimed for the FT-991A only: the FTDX10 has a separate `ST`, and the FTDX101's FA/FB are the MAIN and SUB bands.
- Legacy five-byte files (FT-817ND, FT-857D, FT-897D; family `yaesu-legacy`) use binary transcripts. The PTT-bit level is taken from Hamlib, because the manual's figure is silent (that fact is `hamlib-source`).
- The FT-818ND (`yaesu-ft-818`) is written from its own operating manual and takes the PTT level from it.

### FlexRadio

- Files: `flex-6300`, `flex-6400`, `flex-6600`, `flex-6700`; family `flex-smartsdr`; a `[flex]` block.
- Source: FlexRadio's published SmartSDR API wiki (github.com/flexradio/smartsdr-api-docs; commit and pages named in `crates/radioctl-protocols/src/flex.rs`).
- Its embedded `messages.h` forbids copying and was skipped. There is no 8000-series description because the wiki does not mention that series.
- Control plane only: SmartSDR has no get commands, so the engine subscribes (`sub slice|tx|pan all`) and answers reads from the status it has seen.
- Meters, spectrum and audio are VITA-49 UDP streams (port 4991, `client udpport`, stream ids from the create and `meter list` replies), so the files claim no `read-signal`, `read-power` or `read-swr`.
- Test support:
  - `virtual_flex.rs` is a sans-IO virtual radio (our reading of the wiki, not a model of the real radio);
  - `flex_io.rs` serves it on a TCP listener;
  - `tests/flex_actor.rs` drives it through the TCP transport and the rig actor and checks on the wire that no transmit command is sent while the gate is closed.
- All unverified; the radio was off.

### OpenHPSDR

- Rigs: ANAN-10 and ANAN-100; family `hpsdr-network`; `[hpsdr]` block; files `anan-10` and `anan-100`.
- Control plane only: a line-protocol engine (`hpsdr_net.rs`) over an in-memory stream to a bridge (`hpsdr_io.rs`) that owns an `hpsdr` crate Protocol 1 or 2 session (`daemons/hpsdr`).
- Claims: frequency, PTT, forward power, SWR, and the step attenuator as `Level::Attenuator` (0 to 31 dB; also `HpsdrControl::set_attenuator`).
- No mode (placeholder code only) and no preamplifier.
- PTT passes the actor's closed transmit gate, then the hpsdr crate's own gate, for a bounded lease at drive zero by default.
- All unverified; tested against the loopback emulators in `tests/hpsdr_actor.rs`.

## Rig coverage and volunteer testing

- radioctl aims to describe every Hamlib model (313 in 4.6.2) and to say honestly which ones a person has verified on a radio. The generated list is [docs/RIG_SUPPORT.md](docs/RIG_SUPPORT.md) (`python3 tools/rig_support_matrix.py`; a test fails when it is stale). Today none is hardware-verified.
- The plan, in four tiers with counts and source rules, is [docs/RIG_COVERAGE_PLAN.md](docs/RIG_COVERAGE_PLAN.md).
- To test your own radio, run `radioctl-selftest` (shipped with the hams.com local relay; or build `-p radioctld --bin radioctl-selftest`). It discovers radios, runs only read-only checks, never transmits, writes a local report per radio and uploads each to hams.com (`--no-upload` to skip). See [docs/VOLUNTEER_TESTING.md](docs/VOLUNTEER_TESTING.md) and [docs/RIG_REPORT_UPLOAD.md](docs/RIG_REPORT_UPLOAD.md).
- Hardware reports are recorded in `rigs/reports/verified.toml`; the verification state itself stays in each rig file's facts.

## Tier 1 capability matrix

- `tools/hamlib_caps_matrix.py` runs `rigctl -l` and `rigctl -m N --dump-caps` for every model.
- It writes `data/hamlib-<version>/capability_matrix.json` and `HAMLIB_DIFFERENCES.md`.
- The input is program output only; no Hamlib source is read or copied.
- Parsed fields are under `dump`.
- The protocol family, VFO model, scope flag and quirk hints under `derived` are radioctl's own classification rules (the rule that fired is recorded as `family_basis`). They are planning hints, not statements about a wire protocol.

```
nice -n 10 python3 tools/hamlib_caps_matrix.py --out-dir data/hamlib-4.6.2           # regenerate
nice -n 10 python3 tools/hamlib_caps_matrix.py --check --out-dir data/hamlib-4.6.2   # drift check
cd tools && python3 -m unittest test_hamlib_caps_matrix                            # tests
```

- For a new Hamlib release, run it into a new `data/hamlib-<version>/` directory and diff against the old one.
- Hamlib 4.6.2 lists 313 models (the proposal said 314; that count included the header line).
- 312 models dump; model 2 (NET rigctl) fails with `rig does not have tx_range` and is recorded as not dumpable.

## radioctld

`radioctld --config FILE [--check]` serves the Hamlib `rigctld` text protocol (as documented in the `rigctl`/`rigctld` man pages and as observed from Hamlib 4.6.2's `rigctld`) for any radioctl rig, real or virtual. WSJT-X, fldigi, loggers and `rigctl -m 2` work against it unchanged.

- One `[[rig]]` per radio, each with its own listen address (as with one `rigctld` per radio). See `radioctld.example.toml`.
- Runtime: the `radioctld` binary owns the one multi-thread tokio runtime; the library takes a `Handle`.

### Commands

- Served: `f F m M v V t T s S i I x X j J z Z l L u U b` (`b` is Morse), `1`/`\dump_caps`, `\dump_state`, `\chk_vfo`, `\get_powerstat`, `\get_info`, `\get_lock_mode`, `\get_vfo_info`, `\get_vfo_list`, `\get_split_freq_mode`, `\set_split_freq_mode`, `q`.
- The extended `+` response format is supported.
- Every VFO-taking command accepts the VFO argument or not, in either VFO mode (`vfo_mode`).
- Data modes are `PKTUSB` and `PKTLSB` (`M PKTUSB`).
- Errors:
  - other Hamlib commands: `RPRT -4`;
  - unknown words: `RPRT -1`;
  - what the rig cannot do: `RPRT -11`.
- Never served (`RPRT -11`): the raw `send_cmd` (`w`), power control, and Morse stop.
- Arguments are checked strictly: a missing, surplus or malformed argument is `RPRT -1`, never a wait for another line (Hamlib's `rigctld` waits).
- Full reference: manual appendix C.

### Sharing and PTT

- Several clients share a rig through its single actor queue. Reads come from the actor's cache when newer than `cache_max_age_ms`. No locks around the rig.
- PTT has one owner at a time, using the splitter's `PttOwner`:
  - the first client to key owns the transmitter;
  - another client's `T 1` or `b` is `RPRT -9` (rejected) until the owner unkeys or disconnects (which unkeys the rig);
  - anyone may unkey;
  - Morse (`b`) claims the transmitter the same way, so a Morse sender that disconnects unkeys the rig too;
  - stopping the daemon always unkeys a rig a client was keying.
- Transmit gate:
  - closed unless the configuration says `transmit = true`;
  - transmit commands are refused with `RPRT -9` before anything is queued while the gate is closed or the link is down;
  - `\dump_state` advertises a transmit range only when the gate is open.
- The PTT poll keeps a reserved share of the actor's time under a flooding client.
- A late reply is matched to the command it answers and never taken for the next one.

### Fail closed

- In the actor itself, below every client:
  - a key or Morse command that was written to the rig but never answered (a timeout, a lost reply) is followed at once by an unkey, because the rig may be transmitting;
  - when the last handle to a rig is dropped, the actor unkeys a rig whose cache says keyed or whose gate was open before it ends;
  - a `LineKeyer` makes its line inactive when it is dropped.
- Each is written to the transcript as `FAIL CLOSED unkey`.
- At start-up `radioctld` unkeys every rig, and refuses to serve a rig it cannot unkey.

### Network exposure and bounds

- Every listener binds loopback by default and the allow list defaults to loopback.
- Serving beyond loopback needs `allow_non_loopback_bind = true` together with an explicit `allow` list. This is checked at load; the allow list is applied on accept, before a byte is read.
- Bounds: line length (`max_line_bytes`), clients per rig, idle time, time to finish a started line, write time.
- An over-long line gets `RPRT -1` and the connection is closed.
- The protocol splitter has a TCP client limit and an idle timeout, and an abort closes the stream.

### Serial and TCP rigs

- Serial: `path` or `usb = { vid, pid, serial }`, `baud` (fixed, or `"auto"` / `baud_probe` for the optional read-only probe).
- `lines = "managed"` (default) drives DTR/RTS; `lines = "unmanaged"` is for a pty or other port without DTR/RTS.
- TCP rigs use the reconnecting network link (`net::open_link`):
  - one attempt at start, then reconnection with backoff;
  - the actor marks the link down meanwhile and unkeys the rig when it is back.
- `framing = "ts890-lan"` puts the Kenwood TS-890S LAN session around the CAT stream:
  - `##CN;` and `##ID` login;
  - `ID;` keep-alive every 4 s;
  - silence limit 12 s unless `idle_timeout_ms` says otherwise;
  - `lan_account` names the account;
  - `lan_password_secret` names a file in `server.secrets_dir` (owner-readable only) holding the password, which is never in the configuration file or a message;
  - a refused login or a missing secret stops the daemon from starting.
- Tested against a fake radio on loopback only (`tests/ts890_lan.rs`); unverified on a radio.
- Known gap: after a reconnect the engine's session state (the TS-890S's `AI2;`, a Flex's subscriptions) is not re-sent (manual chapter 13).

### Rotators

- A `[[rotator]]` section serves one rotator in Hamlib's `rotctld` text protocol on its own listener (default port 4533 + index).
- It runs through `radioctl-station`'s rotator engines (`rotctld` and GS-232 models from `data/station/rotators.toml`), over a reconnecting TCP link or against the virtual rotator served inside the daemon.
- Commands: `P p S K _ \dump_state q`, and the `+` format.
- A position outside the model's travel is refused (`RPRT -1`) before anything is written. Other Hamlib rotator commands get `RPRT -4`.
- It uses the same server, allow list and bounds as a rig (`server::LineService`).
- Tested against the virtual rotator and Hamlib's `rotctl -m 2` (`--features hamlib-oracle`) only; unverified on a real rotator.

### Deviations from `rigctld`

All on purpose.

- PTT is released when its owner disconnects.
- The VFO selected by `V` is radioctld's own state (radioctl addresses VFOs explicitly).
- Split is VFO A to receive and VFO B to transmit. `X`/`\set_split_freq_mode` accept only the mode the rig is already in.
- Passbands for a rig with no filter control are the mode's typical width.
- Levels are scaled (0 to 1, or the rig's own integer for keyer speed) by the ranges the engines enforce. The ranges are repeated in `names::level_range` and pinned to the engines by `tests/levels.rs`.
- The CI-V keyer speed is not served because that engine does not carry it.

### Tests

- Protocol transcripts (`tests/transcripts/`).
- Two or more clients, arbitration, and the closed gate (a refused transmit command leaves no byte on the rig's transport).
- The bounds.
- Property tests of the parser and a whole-pipeline fuzz.
- Hamlib `rigctl` as an oracle against a virtual KX3, IC-7300 and TS-590SG (`--features hamlib-oracle`; needs `rigctl` or `RIGCTL=/path`).
- `tests/ts890_lan.rs` for the TS-890S LAN session.

## Using the library

- `use radioctl_protocols::prelude::*;` brings in the usual names (design decision 21).
- `VirtualRig::spawn("icom-ic-7300")` builds a virtual rig and its actor in one call; `open_serial_rig(id, port, baud)` does the same for a serial port.
- Helpers: `level`, `set_level`, `meter`, `start_scope`, `events().next_change(field)`, `transmit_lease(permit, max)`. None opens the transmit gate; the lease takes the permit as an argument.
- The lower-level calls are unchanged. Manual chapter 4 lists every helper; chapter 2 shows the long form.

## Examples

In `crates/radioctld/examples/`; they find `rigs/` next to the binary.

| Example | What it does |
|---|---|
| `quickstart`, `long_form`, `events`, `transmit_gate`, `embed_daemon`, `serial_rig`, `splitter_tcp` | Copied verbatim into manual chapters and built by cargo |
| `rig_file` | Checks one rig description: loads it, lists what is verified, replays its transcript |
| `splitter_pty` | The splitter on virtual serial ports, one pty per outside program |
| `bench_console` | Receive-only console for a first day with a real radio: one command per line on stdin; replies, events and scope frames timestamped and logged. `--unmanaged` for a pty; `--wire FILE` taps every byte in both directions. No key, no Morse, no way to open the gate |
| `virtual_radio_pty` | A rig description's virtual radio on a pty for rehearsing a bench day (usage `virtual_radio_pty RIG_ID LINK [WIRE_LOG]`); stdin commands stand in for the front panel; sends scope sweeps |

## radioctl-station

Own README: [`crates/radioctl-station/README.md`](crates/radioctl-station/README.md); manual chapter 14.

- Radio manager with power button, reconnect backoff, event stream and start-up reconcile; lifecycle states Off, PoweringOn, Booting, Ready, Degraded, Unreachable.
- `ActorLinks` respawns dead rig actors and runs the unkey pass; `min_off_ms` counts from an observed turn-off only.
- `DescriptionSpawner` wires the radio manager to real transports and engines.
- Drivers: Modbus coil relay (SainSmart 16ch), Shelly/Tasmota HTTP, Home Assistant (WebSocket state events, REST service calls) with assurance levels, secret store and audit interfaces.
- Amplifier and tuner channels: operate/standby, tune pulse, fault input.
- Rotators: `rotctld` and GS-232 engines, bounded command handle, virtual rotator.
- Keyed-state interlock (decided by Bruce, 2026-10-31; emergency operations are never blocked):

| Command | Rig known keyed | Keyed state unknown | Unkeyed |
|---|---|---|---|
| Power-off of any device | allowed | allowed | allowed |
| Power-on of a radio; power button on, off, reboot (default) | allowed | allowed | allowed |
| Power-on or reboot of a radio with `shared_supply = true` (opt-in) | refused | refused | allowed |
| Antenna selection, amplifier power-on and operate, band switching, any other output | refused | refused | allowed |
| Emergency `all_off` | allowed | allowed | allowed |

- The station never touches the transmit gate; no keyed state and no power command opens it.
- Unverified on hardware.

## radioctl-capi

- C ABI sketch (own README): explicit runtime ownership, and the transmit gate closed by default.
- Builds as `cdylib` and `staticlib`; header `include/radioctl.h` by cbindgen.
- Unverified sketch.

## Conventions

- **Anchors** (ADR 0090)
  - A function carries `// [@ANCHOR: id]` in source.
  - Its test carries `// Tests [@ANCHOR: id]`.
  - This README cites it as `[@ANCHOR: radioctl:id]` below.
  - `crates/radioctl-core/tests/conventions.rs` fails when source, tests and README disagree, and when a Rust file lacks its SPDX header. A crate may keep its own anchor list in its own README (`radioctl-station` does); the test reads every crate README.
- Protocol engines are sans-IO (see `docs/DESIGN.md`).
- Tests use the same code paths as production; no test-only features.

### Transmit watchdog (baseline 2a.2 to 2a.4)

- Every rig has a maximum transmit duration enforced by its actor, independent of the caller:
  - `RigOptions::max_transmit`, default three minutes (`DEFAULT_MAX_TRANSMIT`);
  - `Duration::MAX` disables it (the owner's decision);
  - counted from the moment the rig is keyed, seen keyed, or sent Morse;
  - `radioctld`: `max_transmit_s` per rig.
- `RigOptions::heartbeat` adds a dead-man period the caller must keep alive with `RigHandle::heartbeat()` while transmitting.
- Either lapse forces an unkey through the fail-closed path and emits an `Alarm` event with `Source::Watchdog`.

### Unkey rules

- Unkeys are retried (three attempts, 100 ms backoff).
- A failed unkey is `Alarm::UnkeyFailed` and counts in `RigStats::unkey_failures`.
- An unkey is never refused for a full emergency queue: it rides on the unkey already queued (`RigStats::unkeys_coalesced`).
- An unkey a caller asked for that the rig did not take (timed out, refused, link down) is also `Alarm::UnkeyFailed` and counted, whatever the caller does with the result. So `RigService::disconnect`, `Splitter::cleanup` and `shutdown` never swallow one.
- An emergency unkey skips inter-command spacing.
- `Op::StopCw` stops Morse on every engine that has a stop path; it is an emergency command like `SetPtt(false)`.

### Links and timing

- A rig file with `timeout_ms = 0` (or above 30 s) or `inter_command_ms` above 2 s is refused at load.
- Loss of the link is `Alarm::LinkDown`, emitted once per loss.
- A serial link's status reaches the actor through `RigOptions::link_watch` (`SerialLink::watch()`). The link is marked down while the port reconnects. When it is back, the actor unkeys the rig before any other command and emits `Alarm::LinkRestored`.
- Network links report the same status.
- A late reply on a rig without `Capability::Transceive` counts in `RigStats::late_replies` and never slows polling.

## Function reference

- [@ANCHOR: radioctl:radioctl_core_vfo_other] (radioctl-core) The other VFO.
- [@ANCHOR: radioctl:radioctl_core_txstate_from_cache] (radioctl-core) The inputs of the transmit-frequency derivation, read from the cached observations.
- [@ANCHOR: radioctl:radioctl_core_txstate_rx_vfo] (radioctl-core) The VFO the rig receives on (the reported one, else VFO A).
- [@ANCHOR: radioctl:radioctl_core_txstate_tx_vfo] (radioctl-core) The VFO the rig transmits on: the receive VFO when simplex, else the reported transmit VFO, else the other VFO.
- [@ANCHOR: radioctl:radioctl_core_txstate_rx_frequency] (radioctl-core) The receive frequency.
- [@ANCHOR: radioctl:radioctl_core_txstate_tx_frequency] (radioctl-core) The frequency of the transmit VFO.
- [@ANCHOR: radioctl:radioctl_core_txstate_active_tx_frequency] (radioctl-core) The frequency the rig transmits on now or when keyed; what amplifiers, tuners and band decoders follow.
- [@ANCHOR: radioctl:radioctl_core_handle_split] (radioctl-core) Whether the rig is in split operation.
- [@ANCHOR: radioctl:radioctl_core_handle_tx_state] (radioctl-core) Brings the inputs of the transmit-frequency derivation up to date.
- [@ANCHOR: radioctl:radioctl_core_handle_tx_vfo] (radioctl-core) The VFO the rig transmits on.
- [@ANCHOR: radioctl:radioctl_core_handle_rx_frequency] (radioctl-core) The receive frequency, independent of which VFO the caller names.
- [@ANCHOR: radioctl:radioctl_core_handle_tx_frequency] (radioctl-core) The transmit VFO's frequency, independent of which VFO the caller names.
- [@ANCHOR: radioctl:radioctl_core_handle_active_tx_frequency] (radioctl-core) The frequency the rig transmits on now, correct in split and while keyed; the status-change stream reports `Field::ActiveTxFrequency` when it changes (Hamlib issue 138).
- [@ANCHOR: radioctl:radioctl_splitter_keyed_split_frequency] (radioctl-splitter) While keyed in split, a plain current-frequency read answers with the transmit frequency (switchable).
- [@ANCHOR: radioctl:radioctld_service_tx_state] (radioctld) The receive and transmit VFOs as the rig last reported them; `\get_active_tx_freq` answers the transmit frequency, `f` keeps the named-VFO meaning.
- [@ANCHOR: radioctl:radioctl_core_txstate_frequency] (radioctl-core) A VFO's cached frequency.
- [@ANCHOR: radioctl:radioctl_kenwood_vfo_of] (radioctl-protocols) The VFO an `FR`/`FT` digit names.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_scope] (radioctl-protocols) The scope operations of the core API (output, axis mode, span, fixed edges, reference level) as CI-V command 27; a span the model does not offer is refused before the wire.
- [@ANCHOR: radioctl:radioctl_core_scope_frame_span] (radioctl-core) The width of a scope sweep's axis.
- [@ANCHOR: radioctl:radioctl_core_scope_frame_centre] (radioctl-core) The centre of a scope sweep's axis.
- [@ANCHOR: radioctl:radioctl_core_handle_scope_frames] (radioctl-core) A stream of the rig's scope sweeps as typed frames, separate from the status-change events; lag is counted.
- [@ANCHOR: radioctl:radioctl_core_scope_stream_next] (radioctl-core) The next scope sweep; frames missed while lagging are skipped and counted.
- [@ANCHOR: radioctl:radioctl_virtual_io_scope_sweep] (radioctl-protocols) Makes a virtual rig send one scope sweep on its own.
- [@ANCHOR: radioctl:radioctl_civ_engine_scope_request_for] (radioctl-protocols) A scope command as a CI-V request with the reply the caller will wait for.
- [@ANCHOR: radioctl:radioctl_scope_axis_from_mode] (radioctl-protocols) The core scope axis for a CI-V scope mode.
- [@ANCHOR: radioctl:radioctl_scope_mode_from_axis] (radioctl-protocols) The CI-V scope mode for a core scope axis.
- [@ANCHOR: radioctl:radioctl_core_hz_from_khz] Builds a frequency in hertz from whole kilohertz, refusing overflow.
- [@ANCHOR: radioctl:radioctl_core_meter_name] (radioctl-core) The name of a raw meter (`swr`, `alc`, `comp`, `vd`, `id`).
- [@ANCHOR: radioctl:radioctl_core_meter_from_name] (radioctl-core) The raw meter with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_level_name] (radioctl-core) The name of a level (AF gain, RF gain, mic gain, monitor level, keyer speed, RF power) in description and transcript files.
- [@ANCHOR: radioctl:radioctl_core_level_from_name] (radioctl-core) The level with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_transcript_step_bytes] (radioctl-core) One direction of a transcript step, as hex or as ASCII text (not both).
- [@ANCHOR: radioctl:radioctl_core_hz_as_mhz] A frequency in megahertz for display.
- [@ANCHOR: radioctl:radioctl_caps_parse_rig_list] Parses `rigctl -l` into model id, manufacturer, model, backend version, status and macro.
- [@ANCHOR: radioctl:radioctl_caps_parse_dump] Parses one `--dump-caps` report: identity, serial settings, timing, VFO, functions, levels, parameters, modes, memories, scope data and the Y/N/E capability flags.
- [@ANCHOR: radioctl:radioctl_caps_parse_ranges] Summarises TX and RX frequency range blocks (count, lowest and highest hertz, highest TX power).
- [@ANCHOR: radioctl:radioctl_caps_classify_family] Derived protocol family from manufacturer, model name and port type, with the rule that fired.
- [@ANCHOR: radioctl:radioctl_caps_vfo_model] Derived VFO model (a_b, main_sub, single, none) from the printed VFO list.
- [@ANCHOR: radioctl:radioctl_caps_quirk_hints] Derived timing and behaviour hints from dump fields only.
- [@ANCHOR: radioctl:radioctl_caps_build_entry] One matrix entry: list row, dump fields and derived fields, or the dump error.
- [@ANCHOR: radioctl:radioctl_caps_build_matrix] Runs rigctl for every listed model and assembles the matrix with provenance.
- [@ANCHOR: radioctl:radioctl_caps_render_markdown] Renders the generated differences document from the matrix.
- [@ANCHOR: radioctl:radioctl_caps_main] Command line: write the files, or `--check` for drift.
- [@ANCHOR: radioctl:radioctl_core_capability_transmits] (radioctl-core) True when using the capability can put RF on the air.
- [@ANCHOR: radioctl:radioctl_core_capability_set_of] (radioctl-core) Builds a set from a list.
- [@ANCHOR: radioctl:radioctl_core_capability_set_has] (radioctl-core) True when the capability is present.
- [@ANCHOR: radioctl:radioctl_core_capability_set_iter] (radioctl-core) The capabilities in a stable order.
- [@ANCHOR: radioctl:radioctl_core_description_mode_code] (radioctl-core) The wire code for a mode, if the model has one.
- [@ANCHOR: radioctl:radioctl_core_description_code_mode] (radioctl-core) The mode for a wire code, if the model has one.
- [@ANCHOR: radioctl:radioctl_core_description_ic7300] (radioctl-core) An Icom IC-7300 style CI-V description (address 0x94). Unverified; see the module note.
- [@ANCHOR: radioctl:radioctl_core_description_ts2000] (radioctl-core) A Kenwood TS-2000 style ASCII description. Unverified; see the module note.
- [@ANCHOR: radioctl:radioctl_alinco_dialect_end] (radioctl-protocols) The dialect end of the alinco code.
- [@ANCHOR: radioctl:radioctl_alinco_command] (radioctl-protocols) The command of the alinco code.
- [@ANCHOR: radioctl:radioctl_drake_encode_frequency] (radioctl-protocols) `F` and seven digits of 10 Hz units, CR; `None` unless a multiple of 10 Hz below 100 MHz.
- [@ANCHOR: radioctl:radioctl_drake_decode_frequency] (radioctl-protocols) The frequency of a `*fffff.ff*uHz` line (without CR LF), or `None`.
- [@ANCHOR: radioctl:radioctl_drake_decode_mode] (radioctl-protocols) The mode of the mode, width and synchronous bytes of an `RM` line, per `description`.
- [@ANCHOR: radioctl:radioctl_drake_engine_new] (radioctl-protocols) Builds an engine from a Drake vendor description.
- [@ANCHOR: radioctl:radioctl_drake_engine_encode] (radioctl-protocols) The engine encode of the drake code.
- [@ANCHOR: radioctl:radioctl_drake_engine_feed] (radioctl-protocols) The engine feed of the drake code.
- [@ANCHOR: radioctl:radioctl_drake_twin_new] (radioctl-protocols) A twin in the description's default state.
- [@ANCHOR: radioctl:radioctl_drake_twin_state_line] (radioctl-protocols) The `RM` answer: three filler bytes, then mode, width and synchronous bytes.
- [@ANCHOR: radioctl:radioctl_drake_twin_feed] (radioctl-protocols) The twin feed of the drake code.
- [@ANCHOR: radioctl:radioctl_drake_twin_front_panel] (radioctl-protocols) The twin front panel of the drake code.
- [@ANCHOR: radioctl:radioctl_alinco_dialect_from_name] (radioctl-protocols) The dialect with a file-format name.
- [@ANCHOR: radioctl:radioctl_alinco_decode_status] (radioctl-protocols) The mode character and receive frequency of a DX-77 status line (26 characters).
- [@ANCHOR: radioctl:radioctl_alinco_engine_new] (radioctl-protocols) Builds an engine from an Alinco vendor description.
- [@ANCHOR: radioctl:radioctl_alinco_decode_reply] (radioctl-protocols) The decode reply of the alinco code.
- [@ANCHOR: radioctl:radioctl_alinco_engine_encode] (radioctl-protocols) The engine encode of the alinco code.
- [@ANCHOR: radioctl:radioctl_alinco_engine_feed] (radioctl-protocols) The engine feed of the alinco code.
- [@ANCHOR: radioctl:radioctl_alinco_twin_new] (radioctl-protocols) A twin in the description's default state.
- [@ANCHOR: radioctl:radioctl_alinco_twin_answer] (radioctl-protocols) The second line (without its CR LF) for a command, or `None` for an unknown command.
- [@ANCHOR: radioctl:radioctl_alinco_twin_feed] (radioctl-protocols) The twin feed of the alinco code.
- [@ANCHOR: radioctl:radioctl_alinco_twin_front_panel] (radioctl-protocols) The twin front panel of the alinco code.
- [@ANCHOR: radioctl:radioctl_racal_format_mhz] (radioctl-protocols) A frequency as a megahertz decimal with up to six decimals and no trailing zeros (`14.2`).
- [@ANCHOR: radioctl:radioctl_racal_parse_mhz] (radioctl-protocols) The frequency of a megahertz decimal (`14.2`, `7`, `0.5`), or `None`.
- [@ANCHOR: radioctl:radioctl_racal_engine_new] (radioctl-protocols) Builds an engine from a Racal vendor description.
- [@ANCHOR: radioctl:radioctl_racal_engine_encode] (radioctl-protocols) The engine encode of the racal code.
- [@ANCHOR: radioctl:radioctl_racal_engine_feed] (radioctl-protocols) The engine feed of the racal code.
- [@ANCHOR: radioctl:radioctl_racal_twin_new] (radioctl-protocols) A twin in the description's default state.
- [@ANCHOR: radioctl:radioctl_racal_twin_feed] (radioctl-protocols) The twin feed of the racal code.
- [@ANCHOR: radioctl:radioctl_racal_twin_front_panel] (radioctl-protocols) The twin front panel of the racal code.
- [@ANCHOR: radioctl:radioctl_tentec_write_reply] (radioctl-protocols) The write reply of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_twin_slot] (radioctl-protocols) The twin slot of the tentec code.
- [@ANCHOR: radioctl:radioctl_core_vendorprotocol_from_name] (radioctl-core) The protocol with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_vendorprotocol_name] (radioctl-core) The file-format name.
- [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_push] (radioctl-protocols) Appends `bytes` received at `now`. Returns the stale partial frame if the buffer held
- [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_take_line] (radioctl-protocols) Removes and returns the first line up to and excluding `terminator`.
- [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_take] (radioctl-protocols) Removes and returns the first `n` bytes if that many have arrived.
- [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_peek] (radioctl-protocols) The bytes held so far.
- [@ANCHOR: radioctl:radioctl_vendor_rxbuffer_take_all] (radioctl-protocols) Removes and returns everything held.
- [@ANCHOR: radioctl:radioctl_vendor_parse_decimal] (radioctl-protocols) The value of a non-empty run of ASCII digits (at most 18 of them), or `None`.
- [@ANCHOR: radioctl:radioctl_vendor_vendor_of] (radioctl-protocols) The vendor protocol of a description, or why it has none.
- [@ANCHOR: radioctl:radioctl_vendor_engine_for] (radioctl-protocols) The engine of a vendor serial description.
- [@ANCHOR: radioctl:radioctl_vendor_twin_for] (radioctl-protocols) The virtual radio of a vendor serial description.
- [@ANCHOR: radioctl:radioctl_tentec_dialect_from_name] (radioctl-protocols) The dialect with a file-format name.
- [@ANCHOR: radioctl:radioctl_tentec_vfo_letter] (radioctl-protocols) The letter of a VFO in Ten-Tec commands.
- [@ANCHOR: radioctl:radioctl_tentec_encode_frequency] (radioctl-protocols) The four big-endian bytes of the binary dialects, or `None` above `u32::MAX` hertz.
- [@ANCHOR: radioctl:radioctl_tentec_decode_frequency] (radioctl-protocols) The frequency of four big-endian bytes, or `None` unless exactly four.
- [@ANCHOR: radioctl:radioctl_tentec_engine_new] (radioctl-protocols) Builds an engine from a Ten-Tec vendor description.
- [@ANCHOR: radioctl:radioctl_tentec_encode_mode_write] (radioctl-protocols) The encode mode write of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_encode_ptt] (radioctl-protocols) The encode ptt of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_frame_len] (radioctl-protocols) Bytes in a binary-dialect answer.
- [@ANCHOR: radioctl:radioctl_tentec_decode_binary] (radioctl-protocols) The decode binary of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_decode_orion_line] (radioctl-protocols) The decode orion line of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_engine_encode] (radioctl-protocols) The engine encode of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_engine_feed] (radioctl-protocols) The engine feed of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_twin_new] (radioctl-protocols) A twin in the description's default state.
- [@ANCHOR: radioctl:radioctl_tentec_twin_orion_line] (radioctl-protocols) The twin orion line of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_twin_binary_frame] (radioctl-protocols) Handles one binary command at the front of the buffer; `None` when more bytes are needed.
- [@ANCHOR: radioctl:radioctl_tentec_twin_feed] (radioctl-protocols) The twin feed of the tentec code.
- [@ANCHOR: radioctl:radioctl_tentec_twin_front_panel] (radioctl-protocols) The twin front panel of the tentec code.
- [@ANCHOR: radioctl:radioctl_core_error_new] (radioctl-core) Builds an error with the default retry hint for its kind and an empty transcript.
- [@ANCHOR: radioctl:radioctl_core_error_with_transcript] (radioctl-core) Attaches the transcript tail.
- [@ANCHOR: radioctl:radioctl_core_observation_field] (radioctl-core) The cache field this observation updates.
- [@ANCHOR: radioctl:radioctl_core_op_capability] (radioctl-core) The capability a rig needs for this command.
- [@ANCHOR: radioctl:radioctl_core_op_transmits] (radioctl-core) True when the command can put RF on the air. Unkeying never transmits.
- [@ANCHOR: radioctl:radioctl_core_op_is_emergency] (radioctl-core) True for the commands that jump the queue: unkeying the transmitter and stopping Morse.
- [@ANCHOR: radioctl:radioctl_core_op_is_unkey] (radioctl-core) True for the unkey alone: the one emergency command that is coalesced when the queue is full and whose failure is `Alarm::UnkeyFailed`.
- [@ANCHOR: radioctl:radioctl_civ_frame_to_bytes] (radioctl-protocols) The wire form of the frame.
- [@ANCHOR: radioctl:radioctl_civ_encode_bcd_freq] (radioctl-protocols) Encodes a frequency as 5 BCD bytes, or `None` above 9,999,999,999 Hz.
- [@ANCHOR: radioctl:radioctl_civ_decode_bcd_freq] (radioctl-protocols) Decodes up to 5 BCD bytes (least-significant pair first); `None` on a non-BCD nibble.
- [@ANCHOR: radioctl:radioctl_civ_decode_bcd_meter] (radioctl-protocols) Decodes a 2-byte BCD meter value, most-significant pair first (`01 20` is 120).
- [@ANCHOR: radioctl:radioctl_civ_encode_bcd_meter] (radioctl-protocols) Encodes a meter value 0 to 9999 as 2 BCD bytes, most-significant pair first.
- [@ANCHOR: radioctl:radioctl_civ_parser_feed] (radioctl-protocols) Adds bytes received at `now` and returns every complete frame and every run of junk.
- [@ANCHOR: radioctl:radioctl_civ_engine_new] (radioctl-protocols) Builds an engine from a CI-V description.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode] (radioctl-protocols) Encodes an Op as a CI-V request with the reply to expect; refuses VFO B, transceive and anything the description lacks.
- [@ANCHOR: radioctl:radioctl_civ_engine_feed] (radioctl-protocols) Decodes CI-V replies and unsolicited transceive frames; skips echoes of our own frames and frames from other stations.
- [@ANCHOR: radioctl:radioctl_kenwood_tokenizer_feed] (radioctl-protocols) Adds bytes received at `now` and returns every complete token.
- [@ANCHOR: radioctl:radioctl_kenwood_parse_digits] (radioctl-protocols) Parses a string of ASCII digits only (no sign, no spaces).
- [@ANCHOR: radioctl:radioctl_kenwood_engine_new] (radioctl-protocols) Builds an engine from a Kenwood-style description.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_encode] (radioctl-protocols) Encodes an Op as a Kenwood ASCII request; refuses PTT readback, power meter and CW.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_feed] (radioctl-protocols) Decodes Kenwood replies, auto-information and `?;` refusals.
- [@ANCHOR: radioctl:radioctl_elecraft_level_wire] (radioctl-protocols) The Elecraft command prefix and manual range of a level.
- [@ANCHOR: radioctl:radioctl_elecraft_signed_offset] (radioctl-protocols) Formats a RIT/XIT offset as `+0050` / `-0050`.
- [@ANCHOR: radioctl:radioctl_elecraft_parse_offset] (radioctl-protocols) Parses `ROsnnnn` data (sign or blank, four digits).
- [@ANCHOR: radioctl:radioctl_elecraft_parse_firmware] (radioctl-protocols) Parses the `NN.NN` revision of an `RVx` answer.
- [@ANCHOR: radioctl:radioctl_elecraft_decode_if] (radioctl-protocols) Decodes an Elecraft `IF` answer into frequency, offset, RIT, XIT, transmit state, mode and split.
- [@ANCHOR: radioctl:radioctl_elecraft_engine_new] (radioctl-protocols) Builds an engine from an Elecraft description.
- [@ANCHOR: radioctl:radioctl_elecraft_engine_encode] (radioctl-protocols) Encodes an Op as an Elecraft command; ranges are checked against the manual, `KY` text and values the engine cannot express are refused.
- [@ANCHOR: radioctl:radioctl_elecraft_encode_set] (radioctl-protocols) The setting commands of the Elecraft engine (frequency, mode, PTT, AI, split, RIT/XIT, filter, levels, CW text).
- [@ANCHOR: radioctl:radioctl_elecraft_check_ky_text] (radioctl-protocols) Accepts `KY` text of 1 to 24 printable characters without `;`, `<` or `>`.
- [@ANCHOR: radioctl:radioctl_elecraft_engine_feed] (radioctl-protocols) Decodes Elecraft replies, `IF` answers, auto-information and `?;`.
- [@ANCHOR: radioctl:radioctl_elecraft_decode] (radioctl-protocols) Decodes one Elecraft reply token into observations.
- [@ANCHOR: radioctl:radioctl_elecraft_decode_one] (radioctl-protocols) Decodes the single-value Elecraft replies (`FA`, `MD`, `TQ`, `SM`, levels, ...).
- [@ANCHOR: radioctl:radioctl_flex_parse_mhz] (radioctl-protocols) Parses a megahertz decimal string to integer hertz without floating point.
- [@ANCHOR: radioctl:radioctl_flex_parse_mhz_signed] (radioctl-protocols) Parses a signed megahertz string (filter edges) to signed hertz.
- [@ANCHOR: radioctl:radioctl_flex_format_mhz] (radioctl-protocols) Formats hertz as megahertz with six decimals.
- [@ANCHOR: radioctl:radioctl_flex_mode_name] (radioctl-protocols) The SmartSDR spelling of a core mode (`digu` for data).
- [@ANCHOR: radioctl:radioctl_flex_mode_from_name] (radioctl-protocols) The core mode for a SmartSDR mode spelling, any case.
- [@ANCHOR: radioctl:radioctl_flex_line_may_transmit] (radioctl-protocols) Classifies a command line as one that can put RF on the air (`xmit`, `transmit tune`, `cwx`, `dvk`, `atu start`).
- [@ANCHOR: radioctl:radioctl_flex_cwx_text] (radioctl-protocols) Quotes CW text for `cwx send` (spaces as 0x7f) and refuses quotes, bars, control and non-ASCII characters.
- [@ANCHOR: radioctl:radioctl_flex_parse_reply] (radioctl-protocols) Decodes `R<seq>|<hex>|message`.
- [@ANCHOR: radioctl:radioctl_flex_parse_pairs] (radioctl-protocols) The `key=value` words of a status line.
- [@ANCHOR: radioctl:radioctl_flex_parse_info] (radioctl-protocols) Decodes the quoted `key=value` list of the `info` reply.
- [@ANCHOR: radioctl:radioctl_flex_parse_firmware] (radioctl-protocols) The SmartSDR-MB major and minor of a `version` reply.
- [@ANCHOR: radioctl:radioctl_flex_parse_discovery] (radioctl-protocols) Decodes a VITA-49 discovery datagram into model, serial, name, address and port.
- [@ANCHOR: radioctl:radioctl_flex_engine_new] (radioctl-protocols) Builds an engine from a FlexRadio description.
- [@ANCHOR: radioctl:radioctl_flex_shadow_value] (radioctl-protocols) The observation the engine's status shadow holds for a field.
- [@ANCHOR: radioctl:radioctl_flex_engine_commands] (radioctl-protocols) Maps an Op to SmartSDR command lines; the transmit ones only for `SetPtt` and `SendCw`.
- [@ANCHOR: radioctl:radioctl_flex_engine_status] (radioctl-protocols) Decodes slice, panadapter, interlock and transmit status lines into observations.
- [@ANCHOR: radioctl:radioctl_flex_engine_line] (radioctl-protocols) Decodes one received line (`V`, `H`, `R`, `S`, `M`).
- [@ANCHOR: radioctl:radioctl_flex_interlock_keyed] (radioctl-protocols) True for interlock states in which RF is or may be on the air.
- [@ANCHOR: radioctl:radioctl_flex_engine_encode] (radioctl-protocols) Encodes an Op as numbered SmartSDR commands, with the subscriptions before the first one.
- [@ANCHOR: radioctl:radioctl_flex_engine_feed] (radioctl-protocols) Splits received bytes into lines (CR, LF or CRLF, bounded) and decodes them.
- [@ANCHOR: radioctl:radioctl_flex_answer_reply] (radioctl-protocols) Turns an `R` reply into an acknowledgement, a refusal, an observation from the shadow, or a wait.
- [@ANCHOR: radioctl:radioctl_flex_apply_interlock] (radioctl-protocols) Applies an interlock status line (state to transmit flag).
- [@ANCHOR: radioctl:radioctl_flex_apply_pan] (radioctl-protocols) Applies a panadapter status line to the panadapter state.
- [@ANCHOR: radioctl:radioctl_flex_apply_slice] (radioctl-protocols) Applies a slice status line to the slice state and returns the observations for VFO A.
- [@ANCHOR: radioctl:radioctl_flex_subscription_lines] (radioctl-protocols) The `sub` or `unsub` lines for slice, tx and pan status.
- [@ANCHOR: radioctl:radioctl_flex_apply_transmit] (radioctl-protocols) Applies a transmit status line (RF power and mic level).
- [@ANCHOR: radioctl:radioctl_virtual_flex_signed_mhz] (radioctl-protocols) Formats signed hertz as signed megahertz for virtual status lines.
- [@ANCHOR: radioctl:radioctl_virtual_flex_new] (radioctl-protocols) Builds a virtual FlexRadio from a Flex description.
- [@ANCHOR: radioctl:radioctl_virtual_flex_command] (radioctl-protocols) Answers one SmartSDR command line (subscriptions, tuning, mode, filter, levels, `xmit`, `cwx`) with one `R` reply and any status.
- [@ANCHOR: radioctl:radioctl_virtual_flex_feed] (radioctl-protocols) The virtual FlexRadio side of the byte stream: `C<seq>|text` lines in, `R` and `S` lines out.
- [@ANCHOR: radioctl:radioctl_virtual_flex_front_panel] (radioctl-protocols) A change made at the virtual radio, pushed as status to subscribers.
- [@ANCHOR: radioctl:radioctl_virtual_flex_serve] (radioctl-protocols) Serves a virtual FlexRadio on a real TCP listener.
- [@ANCHOR: radioctl:radioctl_virtual_flex_control_state] (radioctl-protocols) A copy of the served virtual FlexRadio's state.
- [@ANCHOR: radioctl:radioctl_virtual_flex_interlock_line] (radioctl-protocols) The virtual radio's interlock status line.
- [@ANCHOR: radioctl:radioctl_virtual_flex_line] (radioctl-protocols) Parses one `C<seq>|text` line and answers it.
- [@ANCHOR: radioctl:radioctl_virtual_flex_slice_status_if_subscribed] (radioctl-protocols) The slice status line when the client subscribed to slices, else nothing.
- [@ANCHOR: radioctl:radioctl_virtual_flex_with_status] (radioctl-protocols) Joins a reply and the status it caused in the configured order.
- [@ANCHOR: radioctl:radioctl_virtual_flex_control_front_panel] (radioctl-protocols) Changes something at the served virtual radio; status goes to the client at once.
- [@ANCHOR: radioctl:radioctl_hpsdr_engine_new] (radioctl-protocols) Builds an engine for an `hpsdr-network` description.
- [@ANCHOR: radioctl:radioctl_hpsdr_engine_parse_line] (radioctl-protocols) One complete line from the bridge.
- [@ANCHOR: radioctl:radioctl_hpsdr_engine_encode] (radioctl-protocols) 
- [@ANCHOR: radioctl:radioctl_hpsdr_engine_feed] (radioctl-protocols) 
- [@ANCHOR: radioctl:radioctl_hpsdr_options_default] (radioctl-protocols) A 3 second lease, drive zero, status every 250 ms, the ANAN-100 constant.
- [@ANCHOR: radioctl:radioctl_hpsdr_control_set_attenuator] (radioctl-protocols) Sets the step attenuator in dB (0..=31).
- [@ANCHOR: radioctl:radioctl_hpsdr_control_status] (radioctl-protocols) The latest hardware status of the radio.
- [@ANCHOR: radioctl:radioctl_hpsdr_control_take_streams] (radioctl-protocols) Hands over the receive I/Q streams of the session (the data plane), once.
- [@ANCHOR: radioctl:radioctl_hpsdr_set_attenuator] (radioctl-protocols) Sets the step attenuator of either session, in decibels (the session refuses more than 31).
- [@ANCHOR: radioctl:radioctl_hpsdr_set_frequency] (radioctl-protocols) Retunes DDC0 (and the transmit frequency, which follows it).
- [@ANCHOR: radioctl:radioctl_hpsdr_request_ptt] (radioctl-protocols) Keys or unkeys through the `hpsdr` session, which re-checks its gate before every packet.
- [@ANCHOR: radioctl:radioctl_hpsdr_handle_line] (radioctl-protocols) Answers one request line.
- [@ANCHOR: radioctl:radioctl_hpsdr_status_of] (radioctl-protocols) The latest hardware status of either session.
- [@ANCHOR: radioctl:radioctl_hpsdr_swr_tenths] (radioctl-protocols) SWR in tenths from forward and reverse counts; 10 (1.0:1) with no forward power.
- [@ANCHOR: radioctl:radioctl_hpsdr_serve] (radioctl-protocols) Serves `link` behind an in-memory stream for the rig actor, and returns the control handle. The task
- [@ANCHOR: radioctl:radioctl_hpsdr_virtual_anan] (radioctl-protocols) A virtual ANAN: a Protocol 1 or 2 session on a loopback emulator that the caller started. The gate
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_command] (radioctl-protocols) The virtual Elecraft (KX3) command table.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_set_meta] (radioctl-protocols) Stores a `K2n`/`K3n` meta-command value of the virtual Elecraft rig (Basic formats are still used).
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_level] (radioctl-protocols) Reads or sets a level of the virtual Elecraft rig, clamped to the manual's range.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_if] (radioctl-protocols) The `IF` answer of the virtual Elecraft rig.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_panel] (radioctl-protocols) What the virtual Elecraft rig sends on its own after a panel change, by `AI` mode.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_power] (radioctl-protocols) The virtual Elecraft `PO` reading: nonzero only while keyed.
- [@ANCHOR: radioctl:radioctl_virtual_elecraft_id] (radioctl-protocols) The ID the virtual Elecraft rig reports.
- [@ANCHOR: radioctl:radioctl_virtual_from_description] (radioctl-protocols) Builds a virtual rig of the described model, in its default state.
- [@ANCHOR: radioctl:radioctl_virtual_state] (radioctl-protocols) The rig's current state, for assertions.
- [@ANCHOR: radioctl:radioctl_virtual_feed] (radioctl-protocols) Consumes bytes the controller wrote at `now`; returns the bytes the radio sends back.
- [@ANCHOR: radioctl:radioctl_virtual_front_panel] (radioctl-protocols) Changes state as the radio's own knobs would; returns the unsolicited bytes the rig sends
- [@ANCHOR: radioctl:radioctl_civ_parser_drop_leading_junk] (radioctl-protocols) Reports bytes before any preamble as junk, keeping a trailing lone FE.
- [@ANCHOR: radioctl:radioctl_civ_frame_or_junk] (radioctl-protocols) Turns the bytes FE FE ... FD into a frame or junk; a second preamble restarts the frame.
- [@ANCHOR: radioctl:radioctl_civ_engine_decode] (radioctl-protocols) Decodes one CI-V frame into an Inbound; drops echoes and other stations.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_decode] (radioctl-protocols) Decodes one Kenwood reply token into an Inbound.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_encode_ts440] (radioctl-protocols) The operations whose wire form differs in the TS-440S dialect (IC-10 manual): the mode and transmit VFO read from `IF`, split as `FN0;SP<n>;`, `AI` with one digit, and no exact RIT offset, signal or level.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_encode_ts990] (radioctl-protocols) The operations whose wire form differs in the TS-990S dialect (PC command reference): the mode as `OM0;` and `OM0<m>;`, and no `MD`, `FR`, `FT`, split, PTT read, level or `KY` the engine can use.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_decode_front_end] (radioctl-protocols) An `RA` (attenuator: `P1P1` then an always-zero `P2P2`) or `PA` (preamplifier: `P1` then an always-zero `P2`) answer, mapped to a level by the description's steps.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_front_end_set] (radioctl-protocols) The set command for the attenuator (`RA01;`) or preamplifier (`PA1;`).
- [@ANCHOR: radioctl:radioctl_virtual_state_power] (radioctl-protocols) Power meter of a virtual rig: nonzero only while keyed.
- [@ANCHOR: radioctl:radioctl_virtual_civ_frame] (radioctl-protocols) Answers one CI-V frame: address filter, bus echo, OK/NG.
- [@ANCHOR: radioctl:radioctl_virtual_civ_dispatch] (radioctl-protocols) The virtual CI-V command table (frequency, mode, PTT, CW, ID, meters).
- [@ANCHOR: radioctl:radioctl_virtual_civ_mode_reply] (radioctl-protocols) Mode read reply of the virtual CI-V rig.
- [@ANCHOR: radioctl:radioctl_virtual_stop_cw] (radioctl-protocols) A stop-Morse command (CI-V `17 FF`, Elecraft `KY <EOT>;`, Kenwood `KY0;`) empties the virtual rig's CW buffer and counts the stop.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_command] (radioctl-protocols) The virtual Kenwood command table.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_id] (radioctl-protocols) The ID the virtual Kenwood rig reports.
- [@ANCHOR: radioctl:radioctl_virtual_set_frequency] (radioctl-protocols) Sets VFO A or B from 11 Kenwood digits, else `?;`.
- [@ANCHOR: radioctl:radioctl_virtual_set_kenwood_mode] (radioctl-protocols) Sets the mode from one Kenwood digit, else `?;`.
- [@ANCHOR: radioctl:radioctl_virtual_data_reply] (radioctl-protocols) Builds a CI-V frequency reply.
- [@ANCHOR: radioctl:radioctl_virtual_meter_reply] (radioctl-protocols) Builds a CI-V meter reply (BCD).
- [@ANCHOR: radioctl:radioctl_core_backoff_delay] (radioctl-core) The wait after N failed attempts: exponential, at least 1 ms, capped.
- [@ANCHOR: radioctl:radioctl_core_usb_classify] (radioctl-core) The USB-to-serial chip family (FTDI, CP210x, CH340, CDC ACM) for a vendor and product id.
- [@ANCHOR: radioctl:radioctl_core_usb_port_chip] (radioctl-core) The chip family of a listed port.
- [@ANCHOR: radioctl:radioctl_core_usb_select_port] (radioctl-core) The one port matching vid/pid/serial; none or several are distinct typed errors.
- [@ANCHOR: radioctl:radioctl_core_usb_read_trimmed] (radioctl-core) The trimmed text of a sysfs attribute, or nothing if missing or empty.
- [@ANCHOR: radioctl:radioctl_core_usb_list_sysfs] (radioctl-core) Lists `ttyUSB*`/`ttyACM*` ports with USB ids from a sysfs tree.
- [@ANCHOR: radioctl:radioctl_core_usb_list_ports] (radioctl-core) Lists this machine's serial ports that carry USB ids (sysfs on Linux).
- [@ANCHOR: radioctl:radioctl_core_keyer_new] (radioctl-core) A control-line keyer; makes its line inactive when built.
- [@ANCHOR: radioctl:radioctl_core_keyer_key] (radioctl-core) Keys through the line; refused unless the rig's transmit gate is open.
- [@ANCHOR: radioctl:radioctl_core_keyer_release] (radioctl-core) Unkeys the line; always allowed.
- [@ANCHOR: radioctl:radioctl_core_keyer_drop] (radioctl-core) A dropped keyer makes its line inactive, so a holder that returns or panics while keyed never leaves RF on the air.
- [@ANCHOR: radioctl:radioctl_core_tcp_connect] (radioctl-core) Connects over TCP with a per-attempt timeout and backed-off retries.
- [@ANCHOR: radioctl:radioctl_core_serial_config_new] (radioctl-core) A serial configuration: 8N1, lines managed and inactive, default backoff, one open attempt.
- [@ANCHOR: radioctl:radioctl_core_serial_config_with_description] (radioctl-core) Takes a description's serial line settings (data bits, parity, stop bits, flow control) for the port.
- [@ANCHOR: radioctl:radioctl_core_serial_control_set_line] (radioctl-core) Sets RTS or DTR immediately on the open port.
- [@ANCHOR: radioctl:radioctl_core_serial_link_status] (radioctl-core) The link status: connected (with generation), reconnecting, closed.
- [@ANCHOR: radioctl:radioctl_core_serial_link_watch] (radioctl-core) A receiver that wakes on link status changes.
- [@ANCHOR: radioctl:radioctl_core_serial_link_stats] (radioctl-core) Bytes in, out, dropped while down, and reconnects.
- [@ANCHOR: radioctl:radioctl_core_serial_link_into_parts] (radioctl-core) Splits a link into the byte stream for the actor and a `SerialMonitor` the owner keeps.
- [@ANCHOR: radioctl:radioctl_core_serial_monitor_status] (radioctl-core) The link status from the monitor.
- [@ANCHOR: radioctl:radioctl_core_serial_monitor_watch] (radioctl-core) A status receiver from the monitor.
- [@ANCHOR: radioctl:radioctl_core_serial_monitor_stats] (radioctl-core) The counters from the monitor.
- [@ANCHOR: radioctl:radioctl_core_serial_resolve] (radioctl-core) The device path for a target: the path, or the one port matching the USB ids.
- [@ANCHOR: radioctl:radioctl_core_serial_open_port] (radioctl-core) Opens and configures the port and makes DTR/RTS inactive (or fails).
- [@ANCHOR: radioctl:radioctl_core_serial_write_fully] (radioctl-core) Writes all bytes, riding out short timeouts for about two seconds.
- [@ANCHOR: radioctl:radioctl_core_serial_supervisor_run] (radioctl-core) The supervisor thread: first open, sessions, reconnects with backoff.
- [@ANCHOR: radioctl:radioctl_core_serial_sleep_dropping] (radioctl-core) Waits out a backoff delay, dropping and counting bytes written meanwhile.
- [@ANCHOR: radioctl:radioctl_core_serial_session] (radioctl-core) One connection: the writer loop plus a reader thread.
- [@ANCHOR: radioctl:radioctl_core_serial_read_loop] (radioctl-core) The reader thread: port to async side until stop or failure.
- [@ANCHOR: radioctl:radioctl_core_serial_open] (radioctl-core) Opens a serial port on a caller-supplied runtime handle and returns the byte stream, control lines and status.
- [@ANCHOR: radioctl:radioctl_core_baud_probe_read_request] (radioctl-core) The engine's own frequency read for the baud probe, refused unless its expected reply is the VFO A frequency observation (so the probe can never send a command that changes the radio).
- [@ANCHOR: radioctl:radioctl_core_baud_probe_wait_for_observation] (radioctl-core) Reads the probe's transport for at most one description timeout, feeding the engine; true at the first decoded observation.
- [@ANCHOR: radioctl:radioctl_core_baud_probe_probe] (radioctl-core) The optional baud probe: each configured rate in order, one read-only frequency request and one timeout per rate, stops at the first answer; `LinkDown` naming every rate when none answers. Off unless configured; never touches the transmit gate.
- [@ANCHOR: radioctl:radioctl_core_baud_probe_rates_text] (radioctl-core) A rate list as text for a log line or an error.
- [@ANCHOR: radioctl:radioctl_core_baud_probe_serial_port_new] (radioctl-core) A real serial port as a probe port; the configured rate is replaced at every open.
- [@ANCHOR: radioctl:radioctl_core_baud_probe_serial_port_open] (radioctl-core) Opens the configured device at a probe rate through `open_serial` (same line handling as the real link, one attempt).
- [@ANCHOR: radioctl:radioctl_core_baud_probe_serial_port_close] (radioctl-core) Waits (bounded) until the probe's last link reports `Closed`, so the device is released before the next open.
- [@ANCHOR: radioctl:radioctl_splitter_civ_status_frame] (radioctl-splitter) An OK or NG frame to the sender.
- [@ANCHOR: radioctl:radioctl_splitter_personality] (radioctl-splitter) A new personality for the configured protocol.
- [@ANCHOR: radioctl:radioctl_splitter_set] (radioctl-splitter) A set: asks the rig and reports done or refused.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_code] (radioctl-splitter) The TS-2000 mode digit for a mode.
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_try_key] (radioctl-splitter) Claims the transmitter for `client`. `Ok` when it was free or already theirs; `Err` with the
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_release] (radioctl-splitter) Gives the transmitter up if `client` holds it. Returns whether it did.
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_clear] (radioctl-splitter) Clears ownership because the transmitter was unkeyed by anyone (unkeying is always allowed).
- [@ANCHOR: radioctl:radioctl_splitter_arbiter_owner] (radioctl-splitter) The current owner, if any.
- [@ANCHOR: radioctl:radioctl_splitter_civ_new] (radioctl-splitter) A personality answering at CI-V `address` (0x94 for the IC-7300), with the bus echo on or
- [@ANCHOR: radioctl:radioctl_splitter_civ_mode_code] (radioctl-splitter) The CI-V mode byte for a mode; data modes use the sideband they sit on.
- [@ANCHOR: radioctl:radioctl_splitter_civ_classify] (radioctl-splitter) Turns one frame addressed to us into an action.
- [@ANCHOR: radioctl:radioctl_splitter_civ_parse] (radioctl-splitter) Parses CI-V frames from a client: echoes them if asked, ignores other stations, classifies the rest.
- [@ANCHOR: radioctl:radioctl_splitter_civ_reply] (radioctl-splitter) Renders a CI-V reply (value, OK or NG) addressed to the sender.
- [@ANCHOR: radioctl:radioctl_splitter_civ_unsolicited] (radioctl-splitter) The transceive frames for a VFO A or mode change.
- [@ANCHOR: radioctl:radioctl_splitter_civ_reset] (radioctl-splitter) Forgets a partial frame when the client disconnects.
- [@ANCHOR: radioctl:radioctl_splitter_pty_path] (radioctl-splitter) The path programs should open: the symlink if one was asked for, else the slave device.
- [@ANCHOR: radioctl:radioctl_splitter_pty_slave_path] (radioctl-splitter) The slave device itself (`/dev/pts/N`).
- [@ANCHOR: radioctl:radioctl_splitter_pty_drop] (radioctl-splitter) Stops the endpoint threads and removes the symlink.
- [@ANCHOR: radioctl:radioctl_splitter_pty_attach] (radioctl-splitter) Creates a pty, attaches it to `splitter` as a persistent client, and optionally links
- [@ANCHOR: radioctl:radioctl_splitter_pty_read_master] (radioctl-splitter) Master to async side. A read error is the program closing the slave: report one disconnect and
- [@ANCHOR: radioctl:radioctl_splitter_pty_write_master] (radioctl-splitter) Async side to master. Bytes written while no program has the slave open are dropped.
- [@ANCHOR: radioctl:radioctl_splitter_config_new] (radioctl-splitter) Defaults: cached reads up to 5 seconds old, 2 second deadline for the rig, 16 TCP clients, a 600 second idle timeout (`radioctld`'s limits).
- [@ANCHOR: radioctl:radioctl_splitter_client_wait] (radioctl-splitter) Waits until the client's task has ended.
- [@ANCHOR: radioctl:radioctl_splitter_client_abort] (radioctl-splitter) Stops the client's task and a stream client's reader and writer tasks, so the stream closes. Its PTT ownership is not released by this; drop the stream to
- [@ANCHOR: radioctl:radioctl_splitter_new] (radioctl-splitter) A splitter for `rig`.
- [@ANCHOR: radioctl:radioctl_splitter_stats] (radioctl-splitter) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctl_splitter_ptt_owner] (radioctl-splitter) The client that owns the transmitter, if any.
- [@ANCHOR: radioctl:radioctl_splitter_attach_channels] (radioctl-splitter) Attaches a client given as channels. With `persistent` a `ClientInput::Disconnected`
- [@ANCHOR: radioctl:radioctl_splitter_attach_stream] (radioctl-splitter) Attaches a client that is any byte stream (a TCP connection, an in-memory duplex). The end
- [@ANCHOR: radioctl:radioctl_splitter_serve_tcp] (radioctl-splitter) Accepts connections on `listener` until the returned task is aborted, up to `max_clients` at a time (the rest closed at once, `connections_refused`); each connection is a
- [@ANCHOR: radioctl:radioctl_splitter_cleanup] (radioctl-splitter) Gives up the client's PTT ownership, unkeying the rig if the client held it.
- [@ANCHOR: radioctl:radioctl_splitter_read] (radioctl-splitter) A read: the cached value if fresh enough, else asks the rig.
- [@ANCHOR: radioctl:radioctl_splitter_execute] (radioctl-splitter) Carries out one request for `client`, with PTT arbitration.
- [@ANCHOR: radioctl:radioctl_splitter_send_cw] (radioctl-splitter) Sends Morse for a client, claiming the transmitter like a key does (given back if the rig refuses), so a departing sender unkeys the rig.
- [@ANCHOR: radioctl:radioctl_splitter_key] (radioctl-splitter) Keys for `client` if no other client owns the transmitter; the claim is given back if the
- [@ANCHOR: radioctl:radioctl_splitter_run_client] (radioctl-splitter) The task of one client: input, rig changes, until the client ends.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_new] (radioctl-splitter) A TS-2000 personality with auto-information off.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_classify] (radioctl-splitter) Turns one command (without its `;`) into an action.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_parse] (radioctl-splitter) Splits a client's bytes into TS-2000 commands and classifies each.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_reply] (radioctl-splitter) Renders a TS-2000 reply (`FA`, `MD`, `SM0`, nothing for a set, `?;` for a refusal).
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_unsolicited] (radioctl-splitter) `FA`/`FB`/`MD` pushes, only for a client that switched `AI1` on.
- [@ANCHOR: radioctl:radioctl_splitter_ts2000_reset] (radioctl-splitter) Clears partial input and the auto-information flag on disconnect.
- [@ANCHOR: radioctl:radioctl_core_handle_execute_from] (radioctl-core) Runs a command like `execute_within`, recording the given source on the events it causes (the splitter passes `Splitter`).
- [@ANCHOR: radioctl:radioctl_core_event_merge] (radioctl-core) Folds a later event of the same rig into this one. Per field the earliest `old` and the
- [@ANCHOR: radioctl:radioctl_core_poll_for_capabilities] (radioctl-core) Builds the schedule for the fields the capabilities allow, first due at `now`.
- [@ANCHOR: radioctl:radioctl_core_poll_next_due] (radioctl-core) The earliest due time, if any field is polled.
- [@ANCHOR: radioctl:radioctl_core_poll_take_starved] (radioctl-core) The entry for a field not polled for the reserve (`PollConfig::ptt_reserve`), whatever its own interval: the share run ahead of queued commands.
- [@ANCHOR: radioctl:radioctl_core_poll_take_due] (radioctl-core) The entry to poll now, if one is due. While the rig transmits only transmit state and
- [@ANCHOR: radioctl:radioctl_core_poll_report] (radioctl-core) Records the outcome of polling `field`: a change resets the interval to its minimum, no
- [@ANCHOR: radioctl:radioctl_core_poll_note_transceive] (radioctl-core) Notes that the rig reports on its own; polling slows to the maximum interval.
- [@ANCHOR: radioctl:radioctl_core_poll_interval] (radioctl-core) The current interval of `field`, for tests and diagnostics.
- [@ANCHOR: radioctl:radioctl_core_transmit_permit_new] (radioctl-core) A permit recording who granted it (an operator, an attestation, a test name).
- [@ANCHOR: radioctl:radioctl_core_transmit_permit_granted_by] (radioctl-core) Who granted the permit.
- [@ANCHOR: radioctl:radioctl_core_rig_spawn] (radioctl-core) Spawns a rig actor on a caller-supplied tokio runtime Handle and returns its clonable handle.
- [@ANCHOR: radioctl:radioctl_core_rig_spawn_current] (radioctl-core) Spawns on the runtime of the calling task; fails if there is none.
- [@ANCHOR: radioctl:radioctl_core_rig_owned_shutdown] (radioctl-core) Stops an owned runtime without waiting.
- [@ANCHOR: radioctl:radioctl_core_rig_spawn_with_owned_runtime] (radioctl-core) Feature `owned-runtime` only: builds a one-worker runtime of its own for radioctld and the C ABI.
- [@ANCHOR: radioctl:radioctl_core_handle_id] (radioctl-core) The rig's identifier.
- [@ANCHOR: radioctl:radioctl_core_handle_capabilities] (radioctl-core) What the rig can do.
- [@ANCHOR: radioctl:radioctl_core_handle_open_gate] (radioctl-core) Opens the transmit gate for this rig only. It starts closed.
- [@ANCHOR: radioctl:radioctl_core_handle_close_gate] (radioctl-core) Closes the transmit gate. Transmit commands still queued are refused when they run.
- [@ANCHOR: radioctl:radioctl_core_handle_gate_state] (radioctl-core) Who opened the gate, or `None` while it is closed.
- [@ANCHOR: radioctl:radioctl_core_handle_stats] (radioctl-core) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctl_core_handle_link_up] (radioctl-core) False once the transport has failed.
- [@ANCHOR: radioctl:radioctl_core_handle_heartbeat] (radioctl-core) The caller's dead-man heartbeat: with `RigOptions::heartbeat` set, a keyed rig whose caller stops beating is unkeyed when the period lapses.
- [@ANCHOR: radioctl:radioctl_core_actor_watchdog_due] (radioctl-core) When the transmit watchdog fires while the rig transmits: the earlier of the transmit time limit and the heartbeat lapse.
- [@ANCHOR: radioctl:radioctl_core_actor_watchdog_fire] (radioctl-core) The watchdog lapsed: reports `Alarm::TransmitTimeout` or `Alarm::HeartbeatLapsed`, unkeys through the fail-closed path, disarms.
- [@ANCHOR: radioctl:radioctl_core_actor_arm_watchdog] (radioctl-core) Starts the transmit watchdog when the rig is seen keyed or Morse is sent.
- [@ANCHOR: radioctl:radioctl_core_actor_alarm] (radioctl-core) Emits an alarm event (`Field::Alarm`) attributed to the watchdog or to the caller whose unkey failed, never deduplicated, never cached.
- [@ANCHOR: radioctl:radioctl_core_actor_link_status_changed] (radioctl-core) Applies the transport's link status: down while reconnecting; on a reopen, link up, an unkey pass before any queued command, then `Alarm::LinkRestored`.
- [@ANCHOR: radioctl:radioctl_core_link_changed] (radioctl-core) Waits for the next link status change; false when the watch is closed.
- [@ANCHOR: radioctl:radioctl_core_actor_link_down] (radioctl-core) Marks the transport down and reports the loss once as `Alarm::LinkDown`; every later command is refused with `LinkDown`.
- [@ANCHOR: radioctl:radioctl_core_actor_note_unsolicited] (radioctl-core) A frame nobody waited for: transceive traffic (slows polling) on a rig with `Capability::Transceive`, a counted late reply (`RigStats::late_replies`, polling unchanged) on any other.
- [@ANCHOR: radioctl:radioctl_core_actor_emit] (radioctl-core) Sends an event to the stream subscribers. A broadcast send never waits for a subscriber.
- [@ANCHOR: radioctl:radioctl_core_handle_events] (radioctl-core) The status-change stream, starting now: a `futures_core::Stream<Item = RigEvent>`. Spawn a task on your own runtime to consume it; see [`EventStream`] for lag behaviour.
- [@ANCHOR: radioctl:radioctl_core_handle_cached] (radioctl-core) The cached value of a field with its age, without touching the rig.
- [@ANCHOR: radioctl:radioctl_core_handle_execute] (radioctl-core) Runs a command with the default deadline.
- [@ANCHOR: radioctl:radioctl_core_handle_execute_within] (radioctl-core) Runs a command that must finish within `timeout`. Unkeying the transmitter takes the
- [@ANCHOR: radioctl:radioctl_core_handle_frequency] (radioctl-core) A VFO's frequency.
- [@ANCHOR: radioctl:radioctl_core_handle_set_frequency] (radioctl-core) Sets a VFO's frequency.
- [@ANCHOR: radioctl:radioctl_core_handle_mode] (radioctl-core) The mode.
- [@ANCHOR: radioctl:radioctl_core_handle_set_mode] (radioctl-core) Sets the mode.
- [@ANCHOR: radioctl:radioctl_core_handle_ptt] (radioctl-core) The transmit state.
- [@ANCHOR: radioctl:radioctl_core_handle_set_ptt] (radioctl-core) Keys (needs the transmit gate open) or unkeys (always allowed, jumps the queue).
- [@ANCHOR: radioctl:radioctl_core_handle_signal] (radioctl-core) The S-meter in raw units.
- [@ANCHOR: radioctl:radioctl_core_rigfile_rigs_dir] (radioctl-core) The directory of rig descriptions: `$RADIOCTL_RIGS`, else `./rigs`, else the checkout this library was built from.
- [@ANCHOR: radioctl:radioctl_core_rigfile_load] (radioctl-core) Reads and validates a description by rig id or file path; the error names the file.
- [@ANCHOR: radioctl:radioctl_core_easy_level] (radioctl-core) A numeric level in the rig's own units, from the cache or the radio.
- [@ANCHOR: radioctl:radioctl_core_easy_set_level] (radioctl-core) Sets a numeric level in the rig's own units.
- [@ANCHOR: radioctl:radioctl_core_easy_meter] (radioctl-core) A raw meter (SWR, ALC, COMP, Vd, Id) in the rig's own scale.
- [@ANCHOR: radioctl:radioctl_core_easy_start_scope] (radioctl-core) Subscribes to the scope sweeps, then tells the rig to send them.
- [@ANCHOR: radioctl:radioctl_core_easy_stop_scope] (radioctl-core) Tells the rig to stop sending scope sweeps.
- [@ANCHOR: radioctl:radioctl_core_easy_transmit_lease] (radioctl-core) Opens the transmit gate under an explicit permit for at most a given time; returns the lease.
- [@ANCHOR: radioctl:radioctl_core_easy_next_change] (radioctl-core) Waits for the next event that changes a given field.
- [@ANCHOR: radioctl:radioctl_core_easy_lease_remaining] (radioctl-core) How long a transmit lease has left.
- [@ANCHOR: radioctl:radioctl_core_easy_lease_key] (radioctl-core) Keys the rig inside a lease.
- [@ANCHOR: radioctl:radioctl_core_easy_lease_unkey] (radioctl-core) Unkeys the rig inside a lease.
- [@ANCHOR: radioctl:radioctl_core_easy_lease_transmit_for] (radioctl-core) Keys, holds for a duration (cut short by the lease), and unkeys.
- [@ANCHOR: radioctl:radioctl_core_easy_lease_release] (radioctl-core) Ends the lease now: closes the gate and unkeys.
- [@ANCHOR: radioctl:radioctl_core_easy_lease_drop] (radioctl-core) Ends an unfinished lease on drop: the gate closes and the rig is unkeyed.
- [@ANCHOR: radioctl:radioctl_core_event_stream_poll] (radioctl-core) Polls for the next event, skipping and counting any the subscriber missed.
- [@ANCHOR: radioctl:radioctl_core_event_stream_next] (radioctl-core) The next event, or `None` once the rig has stopped. Cancel-safe: dropping the future loses no event. Events missed while lagging are skipped and counted.
- [@ANCHOR: radioctl:radioctl_core_event_stream_lagged] (radioctl-core) Events this subscriber has missed because it fell behind, in total.
- [@ANCHOR: radioctl:radioctl_core_event_stream_poll_next] (radioctl-core) `Stream::poll_next` for `EventStream`.
- [@ANCHOR: radioctl:radioctl_core_actor_run] (radioctl-core) The actor loop: emergency channel first, then ordinary commands, unsolicited bytes from the rig, and polling last.
- [@ANCHOR: radioctl:radioctl_core_actor_serve] (radioctl-core) Runs one command and answers its caller; counts successes, failures and preemptions.
- [@ANCHOR: radioctl:radioctl_core_actor_run_command] (radioctl-core) Executes one command: deadline and gate checks, write, wait for the reply with retries, preemption by an emergency command, typed errors with the transcript tail.
- [@ANCHOR: radioctl:radioctl_core_actor_observe] (radioctl-core) Updates the cache and, if the value changed, emits an event to the stream subscribers.
- [@ANCHOR: radioctl:radioctl_core_actor_poll_once] (radioctl-core) Polls the next due field at lowest priority.
- [@ANCHOR: radioctl:radioctl_core_actor_poll_entry] (radioctl-core) Reads one polled field and adapts its interval from whether it changed.
- [@ANCHOR: radioctl:radioctl_core_actor_poll_reserved] (radioctl-core) Before a queued command is served, reads the PTT state if none has been read for `ptt_reserve`, so a flooding client cannot hide a radio that keyed or unkeyed on its own.
- [@ANCHOR: radioctl:radioctl_virtual_io_spawn] (radioctl-protocols) Starts `rig` on `runtime`; returns the stream to hand to an actor and a control handle.
- [@ANCHOR: radioctl:radioctl_virtual_io_state] (radioctl-protocols) A copy of the rig's current state.
- [@ANCHOR: radioctl:radioctl_virtual_io_front_panel] (radioctl-protocols) Turns a knob on the rig; any unsolicited bytes go out without delay.
- [@ANCHOR: radioctl:radioctl_virtual_io_set_reply_delay] (radioctl-protocols) Changes how long answers take from now on.
- [@ANCHOR: radioctl:radioctl_virtual_io_drop_replies] (radioctl-protocols) Fault injection: loses the next answers on the way back, the commands still taking effect.
- [@ANCHOR: radioctl:radioctl_virtual_io_next_jitter] (radioctl-protocols) The next random share of the reply jitter, from a seeded generator (repeatable).
- [@ANCHOR: radioctl:radioctl_virtual_io_set_reply_jitter] (radioctl-protocols) Adds a random 0 to `jitter` to every answer's delay (fault injection).
- [@ANCHOR: radioctl:radioctl_virtual_io_set_echo] (radioctl-protocols) Echoes the controller's own bytes back before the answer, as a single-wire bus does (fault injection).
- [@ANCHOR: radioctl:radioctl_virtual_io_set_reply_chunk] (radioctl-protocols) Writes answers in pieces with a gap, so a frame arrives across several reads (fault injection).
- [@ANCHOR: radioctl:radioctl_virtual_io_truncate_replies] (radioctl-protocols) Cuts the last byte off the next answers: a frame that never ends (fault injection).
- [@ANCHOR: radioctl:radioctl_virtual_io_bytes_received] (radioctl-protocols) How many bytes the controller has written to the rig.
- [@ANCHOR: radioctl:radioctl_core_actor_absorb] (radioctl-core) Feeds received bytes to the engine; decides the waiting command by the order rule (a reply an abandoned earlier command is owed is never this command's), updates the cache, counts unrecognised bytes.
- [@ANCHOR: radioctl:radioctl_core_actor_next_seq] (radioctl-core) A fresh identity for one command; its retries share it.
- [@ANCHOR: radioctl:radioctl_core_actor_note_written] (radioctl-core) Remembers a request that expects a reply, for two reply timeouts, so its answer is matched to it even after the command is given up.
- [@ANCHOR: radioctl:radioctl_core_actor_claim_reply] (radioctl-core) Which outstanding request a reply answers: the oldest it fits (Ack to Ack, observation to its field, Nak to any), because the rig answers in order.
- [@ANCHOR: radioctl:radioctl_core_actor_drain_stale] (radioctl-core) Before a normal command is written, absorbs an abandoned command's late reply (or waits for the expectation to lapse) so it is not taken for the new command's answer; emergency commands skip it.
- [@ANCHOR: radioctl:radioctl_core_actor_apply_set] (radioctl-core) Records a successful set in the cache.
- [@ANCHOR: radioctl:radioctl_core_actor_await_reply] (radioctl-core) Waits for the reply until the attempt deadline; an emergency command abandons the wait.
- [@ANCHOR: radioctl:radioctl_core_actor_log] (radioctl-core) Appends to the 16-line transcript kept for error reports.
- [@ANCHOR: radioctl:radioctl_core_actor_spacing] (radioctl-core) Enforces the description's minimum gap between commands; an emergency unkey is exempt.
- [@ANCHOR: radioctl:radioctl_core_handle_read] (radioctl-core) Cached or fresh read of a field, by freshness.
- [@ANCHOR: radioctl:radioctl_core_handle_submit] (radioctl-core) Checks capability and transmit gate, then queues the command without waiting (full queue is a typed error).
- [@ANCHOR: radioctl:radioctl_core_actor_unkey_fail_closed] (radioctl-core) Writes an unkey and waits for its answer, not preemptible, up to three attempts with backoff: used when a transmit command was written but never answered, when the watchdog fires and when the actor stops. An unkey that cannot be done is counted (`unkey_failures`) and reported as `Alarm::UnkeyFailed`.
- [@ANCHOR: radioctl:radioctl_core_actor_unkey_on_stop] (radioctl-core) When the last handle is dropped, a rig whose cache says keyed or whose gate was open is unkeyed before the actor ends.
- [@ANCHOR: radioctl:radioctl_core_deadline_after] (radioctl-core) Saturating `Instant + Duration`: an absurd timeout (a caller's `Duration::MAX`, a rig file's timing) never panics the actor or the caller.
- [@ANCHOR: radioctl:radioctl_core_wait_until] (radioctl-core) Sleeps until the next poll is due, or forever when nothing is polled.
- [@ANCHOR: radioctl:radioctl_core_capability_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_capability_from_name] (radioctl-core) The capability with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_rigfile_parse] (radioctl-core) Parses and validates a description file. Any unknown field, unknown name, duplicate,
- [@ANCHOR: radioctl:radioctl_core_rigfile_expected_keys] (radioctl-core) Every key a description must explain.
- [@ANCHOR: radioctl:radioctl_core_rigfile_fact_for] (radioctl-core) The fact that explains `key`: an exact one if present, else a wildcard.
- [@ANCHOR: radioctl:radioctl_core_rigfile_verification_summary] (radioctl-core) How many keys are in each verified state, keyed by state name.
- [@ANCHOR: radioctl:radioctl_core_rigfile_is_hardware_verified] (radioctl-core) True only when every key of the description is hardware-verified.
- [@ANCHOR: radioctl:radioctl_core_transcript_hex] (radioctl-core) Parses hex bytes written with optional spaces (`fe fe 94` or `fefe94`).
- [@ANCHOR: radioctl:radioctl_core_transcript_parse] (radioctl-core) Parses and validates a transcript file. A step needs either an `op` with its `tx`, or a
- [@ANCHOR: radioctl:radioctl_core_mode_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_mode_from_name] (radioctl-core) The mode with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_vfo_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_vfo_from_name] (radioctl-core) The VFO with a file-format name.
- [@ANCHOR: radioctl:radioctl_conformance_engine_for] (radioctl-protocols) The engine for a description's protocol family.
- [@ANCHOR: radioctl:radioctl_easy_spawn_virtual] (radioctl-protocols) Builds the virtual radio and the engine from a description and spawns the rig actor on a runtime; the gate is closed.
- [@ANCHOR: radioctl:radioctl_easy_virtual_rig_spawn] (radioctl-protocols) A virtual rig of a named model on the current runtime in one call; the gate is closed.
- [@ANCHOR: radioctl:radioctl_easy_open_serial_rig_with] (radioctl-protocols) Opens a serial port, builds the engine, spawns the rig with the link watch and sends the start-up unkey; the gate is closed.
- [@ANCHOR: radioctl:radioctl_easy_open_serial_rig] (radioctl-protocols) A rig from a description id or file and a serial port path, on the current runtime.
- [@ANCHOR: radioctl:radioctl_conformance_replay] (radioctl-protocols) Replays a transcript against the engine and the virtual rig built from one description; reports every byte-level disagreement.
- [@ANCHOR: radioctl:radioctl_conformance_replay_solicited] (radioctl-protocols) Replays one command step: engine encoding, reply expectation and virtual rig answer against the transcript bytes.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_description] (radioctl-core) Builds the RigDescription from the parsed file: family, capabilities, defaults, timing, mode codes.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_facts] (radioctl-core) Builds the provenance facts; duplicates and empty references are errors.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_mode_codes] (radioctl-core) Builds the mode code table; duplicate modes or codes are errors.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_verified] (radioctl-core) Builds a verified state; each state requires its own evidence fields.
- [@ANCHOR: radioctl:radioctl_core_rigfile_check_coverage] (radioctl-core) Every key needs a fact and every fact a key.
- [@ANCHOR: radioctl:radioctl_core_rigfile_matches_key] (radioctl-core) Whether a fact key (exact or prefix wildcard) covers a description key.
- [@ANCHOR: radioctl:radioctl_core_transcript_build_expect] (radioctl-core) Builds the expected decoding of a transcript step.
- [@ANCHOR: radioctl:radioctl_core_transcript_build_op] (radioctl-core) Builds the command of a transcript step.
- [@ANCHOR: radioctl:radioctl_core_transcript_build_panel] (radioctl-core) Builds the knob change of an unsolicited transcript step.
- [@ANCHOR: radioctl:radioctl_conformance_check_decoding] (radioctl-protocols) Checks that the engine decodes a step's rx exactly as the transcript expects.
- [@ANCHOR: radioctl:radioctl_conformance_stand_in_for] (radioctl-protocols) The radio stand-in a transcript replays against: the family's virtual radio, the virtual FlexRadio, or a stand-in for the `hpsdr_io` bridge.
- [@ANCHOR: radioctl:radioctl_conformance_stand_in_prologue] (radioctl-protocols) What the stand-in sends before being asked anything (a FlexRadio's `V` and `H` lines), checked against a transcript's first, op-less step.
- [@ANCHOR: radioctl:radioctl_conformance_stand_in_feed] (radioctl-protocols) The stand-in's answer to the controller's bytes.
- [@ANCHOR: radioctl:radioctl_conformance_stand_in_front_panel] (radioctl-protocols) Turns a knob at the stand-in; names a knob it has not got.
- [@ANCHOR: radioctl:radioctl_conformance_bridge_stand_in_feed] (radioctl-protocols) Answers the hpsdr-network engine's line protocol as the `hpsdr_io` bridge does, with no radio behind it (power 0, SWR 1.0).
- [@ANCHOR: radioctl:radioctl_conformance_expected_inbound] (radioctl-protocols) Maps a transcript expectation to the engine's Inbound value.
- [@ANCHOR: radioctl:radioctl_conformance_panel_change] (radioctl-protocols) Maps a transcript knob change to the virtual rig's PanelChange.
- [@ANCHOR: radioctl:radioctl_core_rigfile_check_timing] (radioctl-core) Refuses a `[timing]` block that would defeat or delay an unkey: `timeout_ms` outside 1..=30000, `inter_command_ms` above 2000.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_levels] (radioctl-core) Validates the optional `[levels]` block: ascending positive attenuator decibels (at most 9) or a whole-decibel maximum, and at most 3 preamplifier steps.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_scope] (radioctl-core) Validates the optional `[scope]` block (icom-civ only; receivers 1 or 2, positive sizes, a span list).
- [@ANCHOR: radioctl:radioctl_civ_engine_scope_request] (radioctl-protocols) The request for a scope command (command 27). Fails when the model has no scope or a value does not fit its field.
- [@ANCHOR: radioctl:radioctl_civ_engine_scope_dropped] (radioctl-protocols) Scope frames or sweeps discarded as out of order or malformed.
- [@ANCHOR: radioctl:radioctl_civ_encode_rit_offset] (radioctl-protocols) The 3 data bytes of the RIT/XIT offset after `21 00`: tens and ones, thousands and hundreds, then a sign byte (1 is negative).
- [@ANCHOR: radioctl:radioctl_civ_decode_rit_offset] (radioctl-protocols) The offset in hertz from the 3 data bytes after `21 00`.
- [@ANCHOR: radioctl:radioctl_civ_filter_index_to_hz] (radioctl-protocols) The width in hertz of filter index `index` (command `1A 03`): 0 to 9 are 50 to 500 Hz in steps of 50, 10 to 40 are 600 to 3600 Hz in steps of 100 (SSB, CW, RTTY); for AM the index 0 to 49 is 200 to 10000 Hz in steps of 200. `None` outside the table.
- [@ANCHOR: radioctl:radioctl_civ_filter_hz_to_index] (radioctl-protocols) The filter index whose width is exactly `hz`, or `None` when the table has no such width.
- [@ANCHOR: radioctl:radioctl_civ_decode_bcd_byte] (radioctl-protocols) One BCD byte (`0x20` is 20); `None` if a nibble is not a digit.
- [@ANCHOR: radioctl:radioctl_civ_engine_decode_filter] (radioctl-protocols) The filter width for index byte `index`, by the mode last reported.
- [@ANCHOR: radioctl:radioctl_civ_engine_decode_scope] (radioctl-protocols) Feeds a waveform frame to the assembler; settings replies are not kept.
- [@ANCHOR: radioctl:radioctl_scope_mode_code] (radioctl-protocols) The wire code of the mode.
- [@ANCHOR: radioctl:radioctl_scope_mode_from_code] (radioctl-protocols) The mode for a wire code.
- [@ANCHOR: radioctl:radioctl_scope_speed_code] (radioctl-protocols) The wire code of the speed.
- [@ANCHOR: radioctl:radioctl_scope_speed_from_code] (radioctl-protocols) The speed for a wire code.
- [@ANCHOR: radioctl:radioctl_scope_command_is_query] (radioctl-protocols) True for the commands that read a setting (the rig answers with data, not OK).
- [@ANCHOR: radioctl:radioctl_scope_command_data] (radioctl-protocols) The sub-command and data bytes (after the `27`) for a scope command, or `None` when a value does not fit its field.
- [@ANCHOR: radioctl:radioctl_scope_decode_reference] (radioctl-protocols) The reference level, in hundredths of a dB, from the 3 data bytes after `27 19 <rx>`.
- [@ANCHOR: radioctl:radioctl_scope_parse_segment] (radioctl-protocols) Decodes the data after `27 00` (receiver, frame number, frame count, then the rest). `None` for a malformed frame.
- [@ANCHOR: radioctl:radioctl_scope_assembler_new] (radioctl-protocols) An assembler for a model's scope.
- [@ANCHOR: radioctl:radioctl_scope_assembler_dropped] (radioctl-protocols) How many frames or sweeps were thrown away as out of order, oversized or short.
- [@ANCHOR: radioctl:radioctl_scope_assembler_note_malformed] (radioctl-protocols) Counts a waveform frame that could not be decoded.
- [@ANCHOR: radioctl:radioctl_scope_assembler_push] (radioctl-protocols) Adds a frame; returns the sweep it completes, if any.
- [@ANCHOR: radioctl:radioctl_scope_sim_new] (radioctl-protocols) A scope in its power-on state: display on, output off, centred, narrowest span.
- [@ANCHOR: radioctl:radioctl_scope_sim_output_on] (radioctl-protocols) True when waveform output is switched on.
- [@ANCHOR: radioctl:radioctl_scope_sim_handle] (radioctl-protocols) Answers a scope command (the bytes after `27`).
- [@ANCHOR: radioctl:radioctl_scope_sim_sweep] (radioctl-protocols) The frames (each `27 00 ...` data, without the CI-V envelope) of one sweep of a receiver whose tuned frequency is `tuned`, or none when output is off, the receiver does not exist or hold is on. Samples are a deterministic ramp that moves each sweep, so a reassembled line can be checked sample by sample.
- [@ANCHOR: radioctl:radioctl_scope_sample_value] (radioctl-protocols) The deterministic synthetic amplitude of pixel `index` in sweep `sweep`.
- [@ANCHOR: radioctl:radioctl_virtual_civ_slot] (radioctl-protocols) The frequency of the selected VFO, or of the other one when `other` is true.
- [@ANCHOR: radioctl:radioctl_virtual_civ_set_freq] (radioctl-protocols) Sets the selected (or the other) VFO from 5 BCD bytes; NG on a malformed field.
- [@ANCHOR: radioctl:radioctl_virtual_civ_front_end_get] (radioctl-protocols) `11` (attenuator, one BCD byte of decibels) and `16 02` (preamplifier step) reads.
- [@ANCHOR: radioctl:radioctl_virtual_civ_front_end_set] (radioctl-protocols) Stores an attenuator (BCD byte) or preamplifier (step) value in a virtual CI-V rig.
- [@ANCHOR: radioctl:radioctl_virtual_civ_level] (radioctl-protocols) Answers `14 <sub>` with the level as 4-digit BCD; NG for a level the rig does not have.
- [@ANCHOR: radioctl:radioctl_virtual_civ_level_set] (radioctl-protocols) Sets a level from `14 <sub> <BCD>`; NG above 255 or on a non-BCD value.
- [@ANCHOR: radioctl:radioctl_virtual_civ_filter] (radioctl-protocols) Answers `1A 03` with the filter index for the current width and mode; NG when the width is not in the mode's table.
- [@ANCHOR: radioctl:radioctl_virtual_civ_filter_set] (radioctl-protocols) Sets the filter width from a `1A 03` index byte, by the mode's table.
- [@ANCHOR: radioctl:radioctl_virtual_scope_sweep] (radioctl-protocols) The bytes the rig sends on its own for one scope sweep of the Main receiver at the VFO A frequency, as broadcast frames; empty when the model has no scope, output is off or the scope is held.
- [@ANCHOR: radioctl:radioctl_virtual_civ_level_of] (radioctl-protocols) The level behind a `14` sub-command (AF 01, RF 02, power 0A, mic 0B, monitor 15).
- [@ANCHOR: radioctl:radioctl_civ_level_sub] (radioctl-protocols) Wire sub-command of a level under `14`, for the levels this engine supports.
- [@ANCHOR: radioctl:radioctl_civ_meter_sub] (radioctl-protocols) Wire sub-command of a raw meter under `15` (SWR 12, ALC 13, COMP 14, Vd 15, Id 16).
- [@ANCHOR: radioctl:radioctl_civ_sub_meter] (radioctl-protocols) The raw meter behind a `15` sub-command.
- [@ANCHOR: radioctl:radioctl_civ_base_mode] (radioctl-protocols) The sideband a data mode sits on and the DATA flag.
- [@ANCHOR: radioctl:radioctl_civ_mode_with_data] (radioctl-protocols) The mode a sideband plus the DATA flag stands for; `None` for DATA on a mode with no data variant.
- [@ANCHOR: radioctl:radioctl_civ_engine_has_data_modes] (radioctl-protocols) True when the description lists `data`: the mode goes through command `26`, which carries the DATA flag.
- [@ANCHOR: radioctl:radioctl_civ_engine_decode_mode] (radioctl-protocols) The mode of a `04`/`01` or `26 00` reply, with the DATA byte, keeping the filter number for the next set.
- [@ANCHOR: radioctl:radioctl_virtual_state_meter] (radioctl-protocols) A raw CI-V meter of the virtual rig: Vd at any time, the transmitter meters only while keyed.
- [@ANCHOR: radioctl:radioctl_virtual_civ_mode_set] (radioctl-protocols) Sets the virtual rig's mode from a code and the DATA flag; `06` lands on the plain mode.
- [@ANCHOR: radioctl:radioctl_civ_engine_filter_table_is_am] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_vfo_b_split] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_rit_xit] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_f8101] (radioctl-protocols) The IC-F8101 frequency set (`1A 35`) and mode read and set (`1A 34`, `1A 36`) of the CI-V engine; see the source and its test.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_front_end] (radioctl-protocols) Sets the attenuator (`11`, one BCD byte of decibels) or the preamplifier (`16 02`, a step number), when the description lists the value.
- [@ANCHOR: radioctl:radioctl_civ_engine_encode_levels_and_filter] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_scope_line_from] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_scope_sim_try_handle] (radioctl-protocols) Internal helper of the CI-V engine or the scope module; see the source and its test.
- [@ANCHOR: radioctl:radioctl_kenwood_engine_decode_if] (radioctl-protocols) The observations in an `IF` answer (without `IF` and `;`): 35 characters, laid out as the module note says. `None` when the length or a field is wrong.
- [@ANCHOR: radioctl:radioctl_kenwood_level_wire] (radioctl-protocols) The wire form of a level. Ranges are the intersection of the TS-2000 and TS-590 guides.
- [@ANCHOR: radioctl:radioctl_kenwood_decode_level] (radioctl-protocols) A level answer: the command name and its digits (`AG0128` is name `AG`, digits `0128`).
- [@ANCHOR: radioctl:radioctl_kenwood_parse_if_offset] (radioctl-protocols) The RIT/XIT offset in the 10 characters that hold P2 and P3 of an `IF` answer: the sign nearest the end, then 4 or 5 digits to the end.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_if] (radioctl-protocols) The IF answer in the TS-2000 or TS-590 layout (38 characters).
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_select_vfo] (radioctl-protocols) Sets the receive (FR) or transmit (FT) VFO; split is when they differ.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_move_offset] (radioctl-protocols) RU/RD: moves the RIT/XIT offset by five digits of hertz; refuses beyond 9999 Hz.
- [@ANCHOR: radioctl:radioctl_virtual_kenwood_level] (radioctl-protocols) A Kenwood level read or set (AG, RG, MG, ML, PC, KS), refusing values outside the guides' ranges.
- [@ANCHOR: radioctl:radioctld_access_normalise] (radioctld) An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) as the IPv4 address it stands for.
- [@ANCHOR: radioctl:radioctld_access_prefix_matches] (radioctld) True when the first `prefix` bits of `a` and `b` agree.
- [@ANCHOR: radioctl:radioctld_access_octets] (radioctld) The address bytes of an IP address.
- [@ANCHOR: radioctl:radioctld_access_loopback_only] (radioctld) The default: this machine only (`127.0.0.0/8` and `::1`).
- [@ANCHOR: radioctl:radioctld_access_parse] (radioctld) Builds a list from entries such as `192.168.1.0/24`, `10.0.0.5` or `fd00::/8`.
- [@ANCHOR: radioctl:radioctld_access_allows] (radioctld) True when `peer` is inside an allowed network.
- [@ANCHOR: radioctl:radioctld_caps_modes] (radioctld) The modes of the description, in a stable order.
- [@ANCHOR: radioctl:radioctld_caps_range_line] (radioctld) One frequency range line of `\dump_state`.
- [@ANCHOR: radioctl:radioctld_caps_gran] (radioctld) The `level_gran` text: min, max and step of every level, zeros for the ones not served.
- [@ANCHOR: radioctl:radioctld_caps_wire_float] (radioctld) A value as the `level_gran` text of Hamlib's `rigctld` writes it for the first levels (AF, RF):
- [@ANCHOR: radioctl:radioctld_caps_wire_int] (radioctld) An integer level value as the text Hamlib's client expects for an integer level: the integer's
- [@ANCHOR: radioctl:radioctld_caps_granularity] (radioctld) Min, max and step of one served level in the form the client parses for that level.
- [@ANCHOR: radioctl:radioctld_caps_preamp_list] (radioctld) The `\dump_state` preamplifier line: the step numbers the model accepts, ended by 0.
- [@ANCHOR: radioctl:radioctld_caps_attenuator_list] (radioctld) The `\dump_state` attenuator line: the decibel values the model accepts, ended by 0.
- [@ANCHOR: radioctl:radioctld_caps_dump_state] (radioctld) The `\dump_state` text.
- [@ANCHOR: radioctl:radioctld_caps_dump_caps] (radioctld) The `\dump_caps` text: a readable summary, ended by a blank line.
- [@ANCHOR: radioctl:radioctld_command_linebuffer_new] (radioctld) A buffer that accepts lines of at most `max` bytes.
- [@ANCHOR: radioctl:radioctld_command_linebuffer_has_partial] (radioctld) True while the start of a line has arrived but its newline has not.
- [@ANCHOR: radioctl:radioctld_command_linebuffer_push] (radioctld) Adds received bytes and returns the lines they completed. A line longer than the limit is
- [@ANCHOR: radioctl:radioctld_command_into_line] (radioctld) Text if the bytes are UTF-8 without control characters (a tab is fine), else invalid.
- [@ANCHOR: radioctl:radioctld_command_resolve] (radioctld) The canonical long name of a command word, and whether radioctld implements it. `None` when
- [@ANCHOR: radioctl:radioctld_command_takes_vfo] (radioctld) True for commands that accept an optional leading VFO token.
- [@ANCHOR: radioctl:radioctld_command_parse_hz] (radioctld) A frequency in hertz from decimal text such as `14074000` or `14074000.000000`; a fraction of
- [@ANCHOR: radioctl:radioctld_command_parse_int] (radioctld) A signed integer of at most 12 characters.
- [@ANCHOR: radioctl:radioctld_command_parse_float] (radioctld) A finite floating-point number of at most 32 characters.
- [@ANCHOR: radioctl:radioctld_command_sanitise] (radioctld) Text safe to echo in a response header: control characters become `?`.
- [@ANCHOR: radioctl:radioctld_command_morse_text] (radioctld) Morse text for `send_morse`: 1 to [`MAX_MORSE_CHARS`] printable ASCII characters.
- [@ANCHOR: radioctl:radioctld_command_mode_and_passband] (radioctld) `mode passband` arguments.
- [@ANCHOR: radioctl:radioctld_command_flag] (radioctld) A 0/1 flag; any nonzero integer is on.
- [@ANCHOR: radioctl:radioctld_command_build] (radioctld) Builds the command `name` from its arguments (the VFO token already taken off). `rest` is the
- [@ANCHOR: radioctl:radioctld_command_take_vfo] (radioctld) Splits an optional leading VFO token off `rest`. For `send_morse` only the unambiguous
- [@ANCHOR: radioctl:radioctld_command_parse_line] (radioctld) Parses one request line. `None` for a blank line (nothing to answer).
- [@ANCHOR: radioctl:radioctld_config_in_range] (radioctld) A value checked against an inclusive range, naming the key in the error.
- [@ANCHOR: radioctl:radioctld_config_server] (radioctld) Validates the `[server]` section.
- [@ANCHOR: radioctl:radioctld_config_transport] (radioctld) Validates a rig's `[rig.transport]`.
- [@ANCHOR: radioctl:radioctld_config_baud_rate] (radioctld) One baud rate within 300..=4000000.
- [@ANCHOR: radioctl:radioctld_config_baud_setting] (radioctld) A serial rig's `baud` and `baud_probe`: a number is a fixed rate and probes nothing (the default); `baud = "auto"` probes the default rates; `baud_probe = [...]` probes that list; a fixed rate with a list, no rate, an empty or repeating list or a rate out of range is an error.
- [@ANCHOR: radioctl:radioctld_config_rig] (radioctld) Validates one `[[rig]]`. `index` picks the default port; `non_loopback_ok` is the server's
- [@ANCHOR: radioctl:radioctld_config_parse] (radioctld) Parses and validates a configuration file's text. Relative description paths are taken
- [@ANCHOR: radioctl:radioctld_daemon_start] (radioctld) Brings up every rig of `config` (descriptions given in the same order) and starts listening.
- [@ANCHOR: radioctl:radioctld_daemon_start_one] (radioctld) Brings up one rig and its listener.
- [@ANCHOR: radioctl:radioctld_daemon_stop] (radioctld) Stops every listener and client, and unkeys any rig whose transmitter a client held.
- [@ANCHOR: radioctl:radioctld_config_check_name] (radioctld) Checks a rig's or rotator's name: 1 to 32 of `[A-Za-z0-9_.-]`.
- [@ANCHOR: radioctl:radioctld_config_listen_address] (radioctld) The listen address of a rig or rotator: the one given, or loopback at `base + index`.
- [@ANCHOR: radioctl:radioctld_config_rotator] (radioctld) Validates one `[[rotator]]`: a known model from the shipped descriptions, a listen address and a transport.
- [@ANCHOR: radioctl:radioctld_config_check_host_port] (radioctld) Checks that a TCP address is `host:port`.
- [@ANCHOR: radioctl:radioctld_daemon_start_rotator] (radioctld) Brings up one rotator and its listener.
- [@ANCHOR: radioctl:radioctld_rotators_build_rotator] (radioctld) Brings up one rotator on `runtime`: engine, reconnecting link (to the rotator, or to a virtual rotator served on a loopback socket), handle, service.
- [@ANCHOR: radioctl:radioctld_rotctl_resolve] (radioctld) The long name of a rotator command word and whether it is served; `None` for a word Hamlib does not know.
- [@ANCHOR: radioctl:radioctld_rotctl_degrees] (radioctld) A finite degree value from an argument.
- [@ANCHOR: radioctl:radioctld_rotctl_parse_line] (radioctld) Parses one `rotctld` request line. `None` for a blank line.
- [@ANCHOR: radioctl:radioctld_rotctl_error_code] (radioctld) The `RPRT` code for a rotator error: a refused position is `-1`, a refused park `-11`.
- [@ANCHOR: radioctl:radioctld_rotctl_service_new] (radioctld) A rotator service for a `Rotator` handle.
- [@ANCHOR: radioctl:radioctld_rotctl_service_rotator] (radioctld) The rotator handle.
- [@ANCHOR: radioctl:radioctld_rotctl_dump_state] (radioctld) The `\dump_state` text in the shape Hamlib 4.6.2's `rotctld` prints and `rotctl -m 2` reads.
- [@ANCHOR: radioctl:radioctld_rotctl_service_execute] (radioctld) Executes one rotator command; every wait is bounded by the description's reply timeout.
- [@ANCHOR: radioctl:radioctld_rotctl_answer] (radioctld) The text to send for one rotator request line, and whether the connection closes after it.
- [@ANCHOR: radioctl:radioctld_rotctl_service_answer] (radioctld) `LineService::answer` for a rotator.
- [@ANCHOR: radioctl:radioctld_rotctl_service_disconnect] (radioctld) `LineService::disconnect` for a rotator: nothing is held per client.
- [@ANCHOR: radioctl:radioctld_server_rig_answer] (radioctld) `LineService::answer` for a rig.
- [@ANCHOR: radioctl:radioctld_server_rig_disconnect] (radioctld) `LineService::disconnect` for a rig: releases the transmitter if the client held it.
- [@ANCHOR: radioctl:radioctld_main_parse_args] (radioctld) Parses the command line (without the program name).
- [@ANCHOR: radioctl:radioctld_main_load] (radioctld) Reads the configuration file and every rig description it names.
- [@ANCHOR: radioctl:radioctld_main_run] (radioctld) Runs the daemon until the process is interrupted.
- [@ANCHOR: radioctl:radioctld_main] (radioctld) The daemon entry point: reads the command line, loads the configuration, runs the daemon on the one runtime it owns.
- [@ANCHOR: radioctl:radioctld_names_mode_from_hamlib] (radioctld) The mode a Hamlib mode name stands for (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_mode_to_hamlib] (radioctld) Hamlib's name for a mode.
- [@ANCHOR: radioctl:radioctld_names_mode_mask] (radioctld) The bit mask of a set of modes, as `\dump_state` writes it.
- [@ANCHOR: radioctl:radioctld_names_mode_bit] (radioctld) The single mask bit of one mode.
- [@ANCHOR: radioctl:radioctld_names_nominal_width] (radioctld) A typical receive bandwidth for the mode, reported when the rig cannot tell its own.
- [@ANCHOR: radioctl:radioctld_names_parse_vfo] (radioctld) The VFO a token names, if it is a VFO name at all (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_vfo_name] (radioctld) Hamlib's name for a VFO.
- [@ANCHOR: radioctl:radioctld_names_level_lookup] (radioctld) Finds a level by its Hamlib name (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_level_range] (radioctld) The rig's own range for a level, when radioctld knows it for the rig's family. Levels of
- [@ANCHOR: radioctl:radioctld_names_served_levels] (radioctld) The levels this rig can serve: its capabilities, and a known range for numeric levels.
- [@ANCHOR: radioctl:radioctld_names_func_lookup] (radioctld) Finds a function by its Hamlib name (case-insensitive).
- [@ANCHOR: radioctl:radioctld_names_func_mask] (radioctld) The `has_get_func` / `has_set_func` mask for a rig that has RIT and XIT or neither.
- [@ANCHOR: radioctl:radioctld_names_level_mask] (radioctld) The `has_get_level` / `has_set_level` mask of a list of levels; `settable_only` leaves out
- [@ANCHOR: radioctl:radioctld_reply_error_code] (radioctld) The `RPRT` code for a failure of the rig actor.
- [@ANCHOR: radioctl:radioctld_reply_vfo_echo] (radioctld) The VFO text a response header echoes: what the client sent, else `currVFO` in VFO mode.
- [@ANCHOR: radioctl:radioctld_reply_header_line] (radioctld) The extended-format header line: `name:` then the VFO and arguments.
- [@ANCHOR: radioctl:radioctld_reply_render] (radioctld) Renders the answer to one request.
- [@ANCHOR: radioctl:radioctld_rigs_boxed_encode] (radioctld) See the source.
- [@ANCHOR: radioctl:radioctld_rigs_boxed_feed] (radioctld) See the source.
- [@ANCHOR: radioctl:radioctld_rigs_load_description] (radioctld) Reads and validates a rig description file.
- [@ANCHOR: radioctl:radioctld_selftest_is_read_only] (radioctld) True only for operations that read; the self-test refuses anything else.
- [@ANCHOR: radioctl:radioctld_selftest_plan_checks] (radioctld) The read-only checks for what a description claims, in a fixed order.
- [@ANCHOR: radioctl:radioctld_selftest_redact_target] (radioctld) The self-test target with a USB serial number removed.
- [@ANCHOR: radioctl:radioctld_selftest_utc_stamp] (radioctld) `YYYY-MM-DD HH:MM:SS UTC` for a Unix time.
- [@ANCHOR: radioctl:radioctld_selftest_render_report] (radioctld) The plain-text self-test report: header, checks, summary, raw wire log.
- [@ANCHOR: radioctl:radioctld_selftest_wire_tap] (radioctld) A tap that writes every byte in both directions as hexadecimal lines.
- [@ANCHOR: radioctl:radioctld_selftest_any_engine_encode] (radioctld) The self-test's boxed engine: encode.
- [@ANCHOR: radioctl:radioctld_selftest_any_engine_feed] (radioctld) The self-test's boxed engine: feed.
- [@ANCHOR: radioctl:radioctld_selftest_parse_target] (radioctld) `virtual`, a serial path, or `usb:VID:PID[:SERIAL]` as a port target.
- [@ANCHOR: radioctl:radioctld_selftest_open] (radioctld) Opens the rig for the self-test with the transmit gate closed.
- [@ANCHOR: radioctl:radioctld_selftest_run_checks] (radioctld) Runs the planned checks; refuses any operation that is not read-only.
- [@ANCHOR: radioctl:radioctld_candidates_highest_state] (radioctld) The highest verified state among a description's facts.
- [@ANCHOR: radioctl:radioctld_candidates_load_summaries] (radioctld) Reads every description in a directory for listings.
- [@ANCHOR: radioctl:radioctld_candidates_squash] (radioctld) Letters and digits only, lower case, for comparing model names.
- [@ANCHOR: radioctl:radioctld_candidates_for_product] (radioctld) The descriptions whose model name a USB product string contains.
- [@ANCHOR: radioctl:radioctld_candidates_render] (radioctld) The `radioctl-selftest --list` text: ports, evidence, descriptions; never a silent pick.
- [@ANCHOR: radioctl:radioctld_builtin_embedded_texts] (radioctld) The embedded descriptions as `(label, text)` pairs; the label is `embedded:ID`.
- [@ANCHOR: radioctl:radioctld_builtin_choose_source] (radioctld) The source of descriptions: `--rigs-dir` wins, then `RADIOCTL_RIGS` (when not empty), else the
- [@ANCHOR: radioctl:radioctld_builtin_load_texts] (radioctld) Every description of `source` as `(label, text)` pairs. A directory is read as `*.toml` files.
- [@ANCHOR: radioctl:radioctld_discovery_not_radio_reason] (radioctld) Why this port is clearly not a radio, or `None`. Only the device's own ids and text are used.
- [@ANCHOR: radioctl:radioctld_discovery_expected_id] (radioctld) The model id the description says the radio answers to an identify read, if it states one.
- [@ANCHOR: radioctl:radioctld_discovery_ask_radio] (radioctld) Asks the radio at `path` whether it is `description`: the baud probe's frequency read at each
- [@ANCHOR: radioctl:radioctld_discovery_discover_ports] (radioctld) Classifies every port. `descriptions` are the loaded descriptions and `rigs` their summaries.
- [@ANCHOR: radioctl:radioctld_discovery_net_from_packet] (radioctld) A network finding from one discovery datagram, matched to a description by model name.
- [@ANCHOR: radioctl:radioctld_discovery_listen_flex] (radioctld) Listens (receive only, nothing is sent) for FlexRadio discovery broadcasts for `window`.
- [@ANCHOR: radioctl:radioctld_discovery_render_table] (radioctld) The discovery table: where, what the device says it is, and what was chosen.
- [@ANCHOR: radioctl:radioctld_discovery_identified] (radioctld) The serial path of a finding that was identified, with its description id.
- [@ANCHOR: radioctl:radioctld_cli_parse_args] (radioctld) Parses the command line (without the program name). `ID PORT [BAUD]` positionals are the
- [@ANCHOR: radioctl:radioctld_cli_parse_baud] (radioctld) `auto`, or a rate between 300 and 4,000,000.
- [@ANCHOR: radioctl:radioctld_cli_find_description] (radioctld) The loaded description with this id, or an error that lists the ids there are.
- [@ANCHOR: radioctl:radioctld_cli_port_identity] (radioctld) The USB identity of the port named by `target` (a path or `usb:VID:PID`), with no serial number.
- [@ANCHOR: radioctl:radioctld_cli_describe_identity] (radioctld) `usb 10c4:ea60, product "..."` for the report; empty when the port has no USB ids.
- [@ANCHOR: radioctl:radioctld_cli_report_path] (radioctld) Where a rig's local report goes: `--out` for a single rig, else
- [@ANCHOR: radioctl:radioctld_cli_render_summary] (radioctld) The run summary: the discovery table and one line per tested rig.
- [@ANCHOR: radioctl:radioctld_cli_choose_targets] (radioctld) The rigs to test: the one named by `--description` and `--port`, or what discovery
- [@ANCHOR: radioctl:radioctld_cli_run] (radioctld) Runs the program for `o`; returns the exit status: 0 when every check of every rig passed or
- [@ANCHOR: radioctl:radioctld_upload_uuid_v4] (radioctld) Formats 16 random bytes as a version 4 UUID.
- [@ANCHOR: radioctl:radioctld_upload_new_report_id] (radioctld) A fresh random report id, or an error if the system has no randomness.
- [@ANCHOR: radioctl:radioctld_upload_rfc3339] (radioctld) `YYYY-MM-DDTHH:MM:SSZ` for a Unix time.
- [@ANCHOR: radioctl:radioctld_upload_build_payload] (radioctld) The upload document, at most [`MAX_BODY`] bytes: raw bytes are dropped from the last check
- [@ANCHOR: radioctl:radioctld_upload_config_dir] (radioctld) The per-user configuration directory for radioctl: `$XDG_CONFIG_HOME` or `~/.config` on
- [@ANCHOR: radioctl:radioctld_upload_load_saved] (radioctld) The saved contact answers, or `None` when the volunteer has not been asked yet.
- [@ANCHOR: radioctl:radioctld_upload_save] (radioctld) Saves the answers in a file only the owner can read (mode 0600 where the system has modes).
- [@ANCHOR: radioctl:radioctld_upload_valid_email] (radioctld) A plain shape check (something, `@`, a dotted domain, no spaces); it does not verify the address.
- [@ANCHOR: radioctl:radioctld_upload_prompt_contact] (radioctld) Asks for the name, then the email, saying why. Enter skips either; an email that does not look
- [@ANCHOR: radioctl:radioctld_upload_resolve_contact] (radioctld) The contact to send. A flag beats an environment variable beats the saved answers. The
- [@ANCHOR: radioctl:radioctld_upload_parse_server] (radioctld) `https://HOST[:PORT]/PATH`, or `http://` only for a loopback host (`localhost`, `127.0.0.1`,
- [@ANCHOR: radioctl:radioctld_upload_post_report] (radioctld) Sends `body` to `url` within [`UPLOAD_TIMEOUT`]. `Ok` only for a 2xx answer; every failure
- [@ANCHOR: radioctl:radioctld_upload_notice] (radioctld) What is said before uploading: what is sent, to where, and where the local copy is.
- [@ANCHOR: radioctl:radioctld_candidates_summaries_from] (radioctld) Parses `(label, description text)` pairs into summaries sorted by id; the label names the
- [@ANCHOR: radioctl:radioctld_candidates_auto_pick] (radioctld) The one port and description the devices themselves name: exactly one USB port whose product
- [@ANCHOR: radioctl:radioctld_selftest_run_checks_logged] (radioctld) [`run_checks`], and when `wire_path` names the wire file the tap writes, each result also
- [@ANCHOR: radioctl:radioctld_cli_firmware_text] (radioctld) The firmware version read from the rig, as text, if any.
- [@ANCHOR: radioctl:radioctld_cli_thanks] (radioctld) The thank-you printed after a successful upload.
- [@ANCHOR: radioctl:radioctld_cli_update_flow] (radioctld) The start-of-run version check. Offline or any error is one line and the test carries on
- [@ANCHOR: radioctl:radioctld_cli_main_from_env] (radioctld) The whole program for a binary's `main`: arguments, terminal, environment and the real serial
- [@ANCHOR: radioctl:radioctld_cli_main_with_bundle] (radioctld) [`main_from_env`] for a program shipped inside a signed release bundle: `bundle` is the bundle's version
- [@ANCHOR: radioctl:radioctld_upload_decode_body] (radioctld) The body of a complete HTTP/1.1 response read to the end of the connection: the bytes after
- [@ANCHOR: radioctl:radioctld_upload_request] (radioctld) `method` (`POST` with a JSON `body`, or `GET` with none) to `url` within `timeout`; returns the
- [@ANCHOR: radioctl:radioctld_update_platform] (radioctld) The release platform name of this build, as the relay's channel calls it, or `None` on a
- [@ANCHOR: radioctl:radioctld_update_is_newer] (radioctld) True when `candidate` is a higher `X.Y.Z` than `current`. Anything else is an error.
- [@ANCHOR: radioctl:radioctld_update_fetch_latest] (radioctld) Asks the release channel for the newest version of `platform` (the relay bridge's
- [@ANCHOR: radioctl:radioctld_update_download] (radioctld) Downloads the signed bundle of `platform` (at most [`MAX_BUNDLE`] bytes, within [`DOWNLOAD_TIMEOUT`]).
- [@ANCHOR: radioctl:radioctld_update_verify_and_extract] (radioctld) The self-test executable out of `bundle`, but only if the bundle's sha256 is `sha256` and its
- [@ANCHOR: radioctl:radioctld_update_replace_exe] (radioctld) Replaces `exe` with `new_bytes`, keeping the old file beside it as `NAME.prev`. The new file is
- [@ANCHOR: radioctl:radioctld_update_restart] (radioctld) Starts the replaced program with the same arguments and returns its exit status (on Unix the
- [@ANCHOR: radioctl:radioctld_cli_probe_baud] (radioctld) Probes the baud for `description` on `path`, the way `baud = "auto"` does.
- [@ANCHOR: radioctl:radioctld_cli_test_one] (radioctld) Tests one rig and writes its local report. `Err` only for a rig that could not be opened.
- [@ANCHOR: radioctl:radioctld_update_entry_name] (radioctld) The file name of the self-test inside a bundle.
- [@ANCHOR: radioctl:radioctld_upload_exchange] (radioctld) One request and the status and body it earned; any network trouble is an `Err` with a short
- [@ANCHOR: radioctl:radioctld_knob_claims_unsolicited] (radioctld) True when the description claims the rig reports changes on its own.
- [@ANCHOR: radioctl:radioctld_knob_watch] (radioctld) Watches the rig's event stream for up to `wait` for a frequency change whose source is
- [@ANCHOR: radioctl:radioctld_knob_result] (radioctld) The report line for an outcome. A knob that was not seen counts as a check that did not pass
- [@ANCHOR: radioctl:radioctld_knob_check] (radioctld) The whole check, or `None` when the description does not claim the capability. Without a
- [@ANCHOR: radioctl:radioctld_cli_stdin_lines_from] (radioctld) Lines from `rx` as a [`BufRead`], and a poll that is true once a line was waiting (and
- [@ANCHOR: radioctl:radioctld_cli_stdin_lines_spawn] (radioctld) Starts the thread that reads the process's standard input line by line.
- [@ANCHOR: radioctl:radioctld_cli_stdin_lines_read] (radioctld) Reads from the current line.
- [@ANCHOR: radioctl:radioctld_cli_stdin_lines_fill_buf] (radioctld) Waits for the next line when the current one is used up; end of input gives no bytes.
- [@ANCHOR: radioctl:radioctl_support_load_descriptions] (tools) Reads rigs/*.toml: name, family, highest verified state, hardware facts.
- [@ANCHOR: radioctl:radioctl_support_load_reports] (tools) The [[report]] entries of rigs/reports/verified.toml.
- [@ANCHOR: radioctl:radioctl_support_check_consistency] (tools) Problems that would make RIG_SUPPORT.md lie: map, facts and reports must agree.
- [@ANCHOR: radioctl:radioctl_support_classify] (tools) Family, tier and engine for a Hamlib model.
- [@ANCHOR: radioctl:radioctl_support_status] (tools) described / engine only / needs variant / needs engine / odd device / not applicable.
- [@ANCHOR: radioctl:radioctl_support_build_rows] (tools) One row per Hamlib model.
- [@ANCHOR: radioctl:radioctl_support_render] (tools) The RIG_SUPPORT.md text.
- [@ANCHOR: radioctl:radioctl_support_generate] (tools) The Markdown for the repository as it is now.
- [@ANCHOR: radioctl:radioctl_support_main] (tools) Writes docs/RIG_SUPPORT.md or checks it with --check.
- [@ANCHOR: radioctl:radioctld_rigs_service_config] (radioctld) The per-rig service settings from the configuration.
- [@ANCHOR: radioctl:radioctld_rigs_build_rig] (radioctld) Brings up one rig on `runtime`: engine, transport, actor, service. The transmit gate is
- [@ANCHOR: radioctl:radioctld_rigs_serial_config] (radioctld) The serial configuration of a rig at a rate: 8N1, the configured line handling, one open attempt; the baud probe and the real link open the port the same way.
- [@ANCHOR: radioctl:radioctld_rigs_tcp_framing] (radioctld) The framing of a TCP rig and its silence limit; for the TS-890S LAN session the password is read from the secret store now, and the error for a missing secret names the secret, never a value.
- [@ANCHOR: radioctl:radioctld_config_tcp_framing] (radioctld) Validates a tcp transport's `framing` and `lan_*` keys: `raw` takes none; `ts890-lan` needs `lan_account`, `lan_password_secret` (a secret name) and `server.secrets_dir`.
- [@ANCHOR: radioctl:radioctld_server_limits_from_config] (radioctld) The limits of a configuration.
- [@ANCHOR: radioctl:radioctld_server_serve] (radioctld) Starts serving `service` on `listener`. The caller binds the listener (loopback unless the
- [@ANCHOR: radioctl:radioctld_server_local_addr] (radioctld) The address clients connect to.
- [@ANCHOR: radioctl:radioctld_server_stats] (radioctld) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctld_server_shutdown] (radioctld) Stops accepting, disconnects every client (a client that owned the transmitter unkeys the
- [@ANCHOR: radioctl:radioctld_server_accept_loop] (radioctld) Accepts connections until aborted. Peers outside the allow-list are dropped before a byte is
- [@ANCHOR: radioctl:radioctld_server_answer] (radioctld) The text to send for one request line, and whether the connection closes after it.
- [@ANCHOR: radioctl:radioctld_server_render_invalid] (radioctld) The answer to a line that is not text.
- [@ANCHOR: radioctl:radioctld_server_write] (radioctld) Writes a reply within the write timeout; false when the client is gone or stalled.
- [@ANCHOR: radioctl:radioctld_server_run_client] (radioctld) The task of one client, until it leaves, misbehaves, or the server stops.
- [@ANCHOR: radioctl:radioctld_service_fmt_float] (radioctld) A level value as text: six decimals with trailing zeros dropped (`0.5`, `1`, `0.117647`).
- [@ANCHOR: radioctl:radioctld_service_new] (radioctld) A service for `rig`, described by `description`.
- [@ANCHOR: radioctl:radioctld_service_rig] (radioctld) The rig's handle.
- [@ANCHOR: radioctl:radioctld_service_config] (radioctld) The service settings.
- [@ANCHOR: radioctl:radioctld_service_stats] (radioctld) A snapshot of the counters.
- [@ANCHOR: radioctl:radioctld_service_ptt_owner] (radioctld) The client that owns the transmitter, if any.
- [@ANCHOR: radioctl:radioctld_service_caps_view] (radioctld) What `\dump_state` and `\dump_caps` are generated from.
- [@ANCHOR: radioctl:radioctld_service_disconnect] (radioctld) A client went away: if it owned the transmitter, give it up and unkey the rig.
- [@ANCHOR: radioctl:radioctld_service_shutdown] (radioctld) The server is stopping: if a client holds the transmitter, unkey the rig.
- [@ANCHOR: radioctl:radioctld_service_exec] (radioctld) Runs one operation on the rig's queue.
- [@ANCHOR: radioctl:radioctld_service_read] (radioctld) A read: the cached value if fresh enough, else asks the rig.
- [@ANCHOR: radioctl:radioctld_service_set] (radioctld) A set: asks the rig and reports done.
- [@ANCHOR: radioctl:radioctld_service_resolve_vfo] (radioctld) The VFO a selection means right now.
- [@ANCHOR: radioctl:radioctld_service_frequency] (radioctld) A VFO's frequency in hertz.
- [@ANCHOR: radioctl:radioctld_service_mode_and_width] (radioctld) The mode and the receive bandwidth in hertz (the mode's typical width when the rig has no
- [@ANCHOR: radioctl:radioctld_service_mode_fields] (radioctld) The `Mode` and `Passband` answer fields, with the given labels.
- [@ANCHOR: radioctl:radioctld_service_set_mode] (radioctld) Sets the mode, and the receive bandwidth when one is asked for and the rig has the
- [@ANCHOR: radioctl:radioctld_service_require_mode] (radioctld) A split transmit mode other than the current one cannot be set: radioctl rigs have one
- [@ANCHOR: radioctl:radioctld_service_refuse] (radioctld) Counts a refused transmit command and answers with the refusal code.
- [@ANCHOR: radioctl:radioctld_service_check_transmit] (radioctld) The fail-closed check in front of every command that can put RF on the air: the gate must
- [@ANCHOR: radioctl:radioctld_service_key] (radioctld) Keys the transmitter for `client` if the gate is open and no other client owns it. The
- [@ANCHOR: radioctl:radioctld_service_unkey] (radioctld) Unkeys the transmitter. Open to every client, gate open or not.
- [@ANCHOR: radioctl:radioctld_service_send_morse] (radioctld) Sends Morse text unless the gate is closed or another client owns the transmitter.
- [@ANCHOR: radioctl:radioctld_service_stop_morse] (radioctld) `\stop_morse`: stops Morse in progress through the rig's emergency path, for any client, owner or not, gate open or closed.
- [@ANCHOR: radioctl:radioctld_service_split_vfo] (radioctld) `s`: whether split is on and which VFO transmits.
- [@ANCHOR: radioctl:radioctld_service_set_split_vfo] (radioctld) `S`: split on means receive on VFO A and transmit on VFO B.
- [@ANCHOR: radioctl:radioctld_service_rit_offset] (radioctld) `j` and `z`: the offset in hertz while the function is on, else 0.
- [@ANCHOR: radioctl:radioctld_service_set_rit_offset] (radioctld) `J` and `Z`: 0 switches the function off; any other offset sets it and switches on.
- [@ANCHOR: radioctl:radioctld_service_served_level] (radioctld) The level info if this rig serves the named level.
- [@ANCHOR: radioctl:radioctld_service_get_level] (radioctld) `l`: a level as the client sees it (a 0 to 1 fraction, or the rig's own integer).
- [@ANCHOR: radioctl:radioctld_service_set_level] (radioctld) `L`: sets a numeric level; meters are read-only.
- [@ANCHOR: radioctl:radioctld_service_get_func] (radioctld) `u`: RIT or XIT on or off.
- [@ANCHOR: radioctl:radioctld_service_set_func] (radioctld) `U`: RIT or XIT on or off.
- [@ANCHOR: radioctl:radioctld_service_vfo_info] (radioctld) `\get_vfo_info`: frequency, mode, width and split of the selected VFO.
- [@ANCHOR: radioctl:radioctld_service_execute] (radioctld) Executes one command for `client`. The answer is the body of a success, or the `RPRT`
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_command] (radioctl-protocols) The newcat command interpreter of a virtual Yaesu (FA/FB, MD0, TX, FT/ST, RT/XT/RC/RU/RD, SM0, RM5, IF, ID, AI, levels and the fixed answers Hamlib needs).
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_digits] (radioctl-protocols) Digits of the virtual Yaesu rig's frequency fields: 9, or 8 for the older models (`yaesu.frequency_digits`).
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_set_frequency] (radioctl-protocols) Sets VFO A or B from the model's digits (nine, or eight on the older models); `?;` otherwise.
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_if] (radioctl-protocols) The 28-character IF answer.
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_move_offset] (radioctl-protocols) RU/RD: moves the clarifier offset by four digits of hertz; refuses beyond 9999 Hz.
- [@ANCHOR: radioctl:radioctl_virtual_yaesu_level] (radioctl-protocols) A newcat level read or set (AG0, RG0, MG, PC, KS), refusing values outside the guides' ranges.
- [@ANCHOR: radioctl:radioctl_yaesu_level_wire] (radioctl-protocols) The wire form and range of a level for the newcat family.
- [@ANCHOR: radioctl:radioctl_yaesu_decode_level] (radioctl-protocols) A newcat level answer (command name and digits).
- [@ANCHOR: radioctl:radioctl_yaesu_parse_clarifier] (radioctl-protocols) The clarifier offset in the five characters of IF P3: a sign and four digits.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_new] (radioctl-protocols) Builds a newcat engine from a yaesu-newcat description.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_decode_front_end] (radioctl-protocols) An `RA` or `PA` answer: `0` (main receiver; the FT-991A has this digit as a fixed zero) and the step, mapped by the description.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_decode] (radioctl-protocols) Decodes one newcat reply into observations.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_digits] (radioctl-protocols) Digits of the frequency fields of `FA`, `FB` and `IF` for this model: 9, or 8 on the FT-450, FT-950, FT-2000, FTDX1200, FTDX3000, FTDX5000 and FTDX9000.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_decode_if] (radioctl-protocols) The observations in an IF answer (frequency of VFO A, mode, RIT, XIT, clarifier offset).
- [@ANCHOR: radioctl:radioctl_yaesu_engine_encode] (radioctl-protocols) Encodes an operation as a newcat command.
- [@ANCHOR: radioctl:radioctl_yaesu_engine_feed] (radioctl-protocols) Feeds received bytes to the newcat engine.
- [@ANCHOR: radioctl:radioctl_virtual_legacy_block] (radioctl-protocols) One five-byte block of a virtual FT-817/857/897: reads answer, settings change state, unknown opcodes are silent.
- [@ANCHOR: radioctl:radioctl_ylegacy_encode_bcd_frequency] (radioctl-protocols) The four BCD bytes of a frequency in 10 Hz units (multiples of 10 Hz below 1 GHz).
- [@ANCHOR: radioctl:radioctl_ylegacy_encode_frequency] (radioctl-protocols) The four frequency bytes, with the FT-736R's first-nibble C form above 1.2 GHz when the description asks for it.
- [@ANCHOR: radioctl:radioctl_ylegacy_decode_frequency] (radioctl-protocols) The inverse of the above.
- [@ANCHOR: radioctl:radioctl_ylegacy_decode_bcd_frequency] (radioctl-protocols) The frequency from four BCD bytes of 10 Hz units.
- [@ANCHOR: radioctl:radioctl_ylegacy_engine_new] (radioctl-protocols) Builds a legacy five-byte engine from a yaesu-legacy description.
- [@ANCHOR: radioctl:radioctl_ylegacy_decode_frequency_mode] (radioctl-protocols) The frequency of VFO A and the mode from a five-byte 03 answer.
- [@ANCHOR: radioctl:radioctl_ylegacy_engine_encode] (radioctl-protocols) Encodes an operation as a five-byte block and remembers which answer to count bytes for.
- [@ANCHOR: radioctl:radioctl_ylegacy_engine_feed] (radioctl-protocols) Counts answer bytes (5 for frequency and mode, 1 for a status read) and decodes them.
- [@ANCHOR: radioctl:radioctl_core_levelparams_max_attenuation] (radioctl-core) The largest attenuation in decibels, `None` when the model has no attenuator.
- [@ANCHOR: radioctl:radioctl_core_levelparams_attenuator_step] (radioctl-core) The 1-based step whose attenuation is `db`, or 0 for `db == 0` (off); `None` if no step has that attenuation.
- [@ANCHOR: radioctl:radioctl_core_levelparams_attenuator_db] (radioctl-core) The attenuation in decibels of 1-based step `step` (0 is off, 0 dB); `None` for a step the model does not have.
- [@ANCHOR: radioctl:radioctl_core_levelparams_lacks] (radioctl-core) True when the description lists a level as absent from the model (`levels.absent`).
- [@ANCHOR: radioctl:radioctl_core_civparams_field_len] (radioctl-core) The number of BCD bytes of a CI-V frequency field for a given frequency on this model (4, 5, or 5/6 for the IC-905).
- [@ANCHOR: radioctl:radioctl_civ_encode_bcd_freq_len] (radioctl-protocols) Encodes a frequency as 4, 5 or 6 BCD bytes.
- [@ANCHOR: radioctl:radioctl_civ_engine_freq_bytes] (radioctl-protocols) The frequency as the BCD field of this model.
- [@ANCHOR: radioctl:radioctl_virtual_civ_freq_field] (radioctl-protocols) The virtual rig's VFO frequency as the model's BCD field.
- [@ANCHOR: radioctl:radioctl_att_is_level] (radioctl-protocols) True for the two levels this module handles.
- [@ANCHOR: radioctl:radioctl_att_step] (radioctl-protocols) The wire step of a front-end level value: for the attenuator the 1-based step whose attenuation is `value` dB (0 is off), for the preamplifier the step itself. `Unsupported` when the model has no such control or no such step.  # Errors  `Unsupported` for a level that is not a front-end level, a model without the control and a value the model has no step for.
- [@ANCHOR: radioctl:radioctl_att_value] (radioctl-protocols) The level value of a wire step reported by the rig, the inverse of [`step_of`]; `None` for a step the description does not have (the reply is then unrecognised, not guessed at).
- [@ANCHOR: radioctl:radioctl_att_virtual_get] (radioctl-protocols) What a virtual rig answers to a read of a front-end level: its stored value, `None` when the model has no such control.
- [@ANCHOR: radioctl:radioctl_att_virtual_set] (radioctl-protocols) Stores a front-end level in a virtual rig; false (nothing stored) for a value the model has no step for.
- [@ANCHOR: radioctl:radioctl_att_virtual_ascii] (radioctl-protocols) The shared body of the Kenwood-style and Elecraft virtual `RA`/`PA` handlers; `second_field` appends the always-zero second parameter Kenwood answers carry.
- [@ANCHOR: radioctl:radioctl_att_virtual_kenwood] (radioctl-protocols) The `RA` and `PA` commands of a virtual Kenwood-style rig: `RA;` answers `RAnn00;` (step, then the always-zero second field) and `RAnn;` sets; `PA;` answers `PAn0;` and `PAn;` sets. A refusal is `?;`.
- [@ANCHOR: radioctl:radioctl_att_virtual_elecraft] (radioctl-protocols) The `RA` and `PA` commands of a virtual Elecraft rig: `RA;` answers `RAnn;`, `PA;` answers `PAn;` and the same forms set (Programmer's Reference Rev. G5, p. 22). A refusal is `?;`.
- [@ANCHOR: radioctl:radioctl_att_virtual_yaesu] (radioctl-protocols) The `RA` and `PA` commands of a virtual Yaesu newcat rig: `RA0;` answers `RA0n;` (the leading 0 is the main receiver) and `RA0n;` sets; the same for `PA`. A refusal is `?;`.
- [@ANCHOR: radioctl:radioctl_core_kenwood_dialect_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_kenwood_dialect_from_name] (radioctl-core) The dialect with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_hh_dialect_name] (radioctl-core) The name of a Kenwood handheld dialect in rig description files.
- [@ANCHOR: radioctl:radioctl_core_hh_dialect_from_name] (radioctl-core) The Kenwood handheld dialect with a file-format name.
- [@ANCHOR: radioctl:radioctl_hh_fo_layout] (radioctl-protocols) The line length and mode column of a dialect's `FO` line.
- [@ANCHOR: radioctl:radioctl_hh_th_step_and_frequency] (radioctl-protocols) The frequency and step code an `FQ` set sends (nearer of 5 and 6.25 kHz, 10 kHz from 470 MHz).
- [@ANCHOR: radioctl:radioctl_hh_engine_new] (radioctl-protocols) Builds the Kenwood handheld engine from a kenwood-hh description.
- [@ANCHOR: radioctl:radioctl_hh_engine_encode] (radioctl-protocols) Encodes an op as a CR-ended handheld command.
- [@ANCHOR: radioctl:radioctl_hh_engine_feed] (radioctl-protocols) Splits reply bytes into CR-ended lines.
- [@ANCHOR: radioctl:radioctl_hh_decode_line] (radioctl-protocols) Decodes one reply line against what was asked.
- [@ANCHOR: radioctl:radioctl_hh_parse_fo] (radioctl-protocols) Reads the frequency and mode digit out of an `FO` line.
- [@ANCHOR: radioctl:radioctl_hh_sim_new] (radioctl-protocols) A virtual handheld in its default state.
- [@ANCHOR: radioctl:radioctl_hh_sim_fo_line] (radioctl-protocols) The virtual handheld's `FO` line for a band.
- [@ANCHOR: radioctl:radioctl_hh_sim_feed] (radioctl-protocols) The virtual handheld's answers to command lines.
- [@ANCHOR: radioctl:radioctl_hh_command] (radioctl-protocols) A handheld command line ended by a carriage return.
- [@ANCHOR: radioctl:radioctl_hh_edited_line] (radioctl-protocols) The remembered `FO` line of a band with one edit applied.
- [@ANCHOR: radioctl:radioctl_hh_sim_line] (radioctl-protocols) The virtual handheld's answer to one command line.
- [@ANCHOR: radioctl:radioctl_ts890_level_wire] (radioctl-protocols) The wire form of a level, or `None` where the guide has no such level command here.
- [@ANCHOR: radioctl:radioctl_ts890_lan_connect_request] (radioctl-protocols) The first LAN frame: `##CN;` asks the radio for a connection (guide p. 71); it answers `##CN1;` (authorised) or `##CN0;` (denied).
- [@ANCHOR: radioctl:radioctl_ts890_lan_login] (radioctl-protocols) The second LAN frame, `##ID<type><len><len><account><password>;` (guide p. 72): `administrator` picks account type 0 (administrator) or 1 (user); each length is two digits, 1 to 32.  # Errors  `Unsupported` for an empty or over-long account or password, or one that holds a `;` or a non-ASCII byte (the frame could not be told apart from its terminator).
- [@ANCHOR: radioctl:radioctl_ts890_lan_reply] (radioctl-protocols) What a LAN answer says: `Some(true)` for `##CN1;` and `##ID1;` (authorised), `Some(false)` for `##CN0;` and `##ID0;`, `None` for anything else.
- [@ANCHOR: radioctl:radioctl_ts890_engine_new] (radioctl-protocols) Builds an engine from a Kenwood-family description whose dialect is `ts890`.  # Errors  `Unsupported` for any other description.
- [@ANCHOR: radioctl:radioctl_ts890_engine_decode] (radioctl-protocols) See the source.
- [@ANCHOR: radioctl:radioctl_ts890_engine_decode_step] (radioctl-protocols) An `RA` or `PA` answer: one digit, the step, mapped by the description.
- [@ANCHOR: radioctl:radioctl_ts890_vfo_of] (radioctl-protocols) The VFO named by an `FR`/`FT` digit (`0` is A).
- [@ANCHOR: radioctl:radioctl_ts890_decode_offset] (radioctl-protocols) The `RF` answer after its name: a direction digit (0 plus, 1 minus) and four digits of hertz.
- [@ANCHOR: radioctl:radioctl_ts890_decode_level] (radioctl-protocols) A numeric level answer: the command name and its three digits (`AG128`).
- [@ANCHOR: radioctl:radioctl_ts890_engine_encode] (radioctl-protocols) See the source.
- [@ANCHOR: radioctl:radioctl_ts890_engine_feed] (radioctl-protocols) See the source.
- [@ANCHOR: radioctl:radioctl_virtual_ts440_is] (radioctl-protocols) True when this rig speaks the TS-440S dialect.
- [@ANCHOR: radioctl:radioctl_virtual_ts440_is_fn_sp] (radioctl-protocols) True for the TS-440S (`FN` and `SP`), false for the TS-450S dialect.
- [@ANCHOR: radioctl:radioctl_virtual_ts440_command] (radioctl-protocols) One TS-440S command; the reply bytes (none for a setting, `?;` for a refusal).
- [@ANCHOR: radioctl:radioctl_virtual_ts990_is] (radioctl-protocols) True when this rig speaks the TS-990S dialect.
- [@ANCHOR: radioctl:radioctl_virtual_ts990_command] (radioctl-protocols) The TS-990S forms that differ from the classic table (`OM0`, and refusals); `None` lets the classic table answer.
- [@ANCHOR: radioctl:radioctl_virtual_ts890_is] (radioctl-protocols) True when this rig speaks the TS-890S dialect.
- [@ANCHOR: radioctl:radioctl_virtual_ts890_command] (radioctl-protocols) One TS-890S command; the reply bytes (none for a setting, `?;` for a refusal).
- [@ANCHOR: radioctl:radioctl_virtual_ts890_level] (radioctl-protocols) `AGnnn`, `RGnnn`, `MGnnn`, `KSnnn`, `PCnnn`: a read when `arg` is empty, else a set inside the guide's range.
- [@ANCHOR: radioctl:radioctl_core_receiver_protocol_name] (radioctl-core) The name used in rig description files.
- [@ANCHOR: radioctl:radioctl_core_receiver_protocol_from_name] (radioctl-core) The protocol with a file-format name.
- [@ANCHOR: radioctl:radioctl_core_rigfile_build_receiver] (radioctl-core) Core rigfile build receiver.
- [@ANCHOR: radioctl:radioctl_receiver_engine_new] (radioctl-protocols) Builds the engine of a [`Family::Receiver`] description of protocol `protocol`.
- [@ANCHOR: radioctl:radioctl_receiver_engine_split_lines] (radioctl-protocols) Takes every complete line out of the buffer: the text without line feeds and spaces at
- [@ANCHOR: radioctl:radioctl_receiver_engine_description] (radioctl-protocols) Receiver engine description.
- [@ANCHOR: radioctl:radioctl_receiver_engine_encode] (radioctl-protocols) Receiver engine encode.
- [@ANCHOR: radioctl:radioctl_receiver_engine_feed] (radioctl-protocols) Receiver engine feed.
- [@ANCHOR: radioctl:radioctl_receiver_virtual_new] (radioctl-protocols) The virtual radio of a description (the twin of its protocol).
- [@ANCHOR: radioctl:radioctl_receiver_virtual_feed] (radioctl-protocols) Consumes bytes the controller wrote; the bytes the radio answers.
- [@ANCHOR: radioctl:radioctl_aor_engine] (radioctl-protocols) The engine for an AOR description.
- [@ANCHOR: radioctl:radioctl_aor_dialect_from_description] (radioctl-protocols) Aor dialect from description.
- [@ANCHOR: radioctl:radioctl_aor_dialect_encode] (radioctl-protocols) Aor dialect encode.
- [@ANCHOR: radioctl:radioctl_aor_dialect_decode] (radioctl-protocols) Aor dialect decode.
- [@ANCHOR: radioctl:radioctl_aor_tagged_frequency] (radioctl-protocols) The frequency after the first `RF` (or, on the AR8000, `VA` or `VB`) in a line.
- [@ANCHOR: radioctl:radioctl_aor_signal_reading] (radioctl-protocols) The raw signal value of an `LM` or `LB` line, `None` if the line is not one.
- [@ANCHOR: radioctl:radioctl_aor_round_to_50_hz] (radioctl-protocols) A frequency rounded as the AOR command needs it (a multiple of 50 Hz), `None` above the ten
- [@ANCHOR: radioctl:radioctl_aor_twin_new] (radioctl-protocols) The twin of an AOR description.
- [@ANCHOR: radioctl:radioctl_aor_twin_reply] (radioctl-protocols) A reply line: a leading line feed and the closing CR, as Hamlib reads them.
- [@ANCHOR: radioctl:radioctl_aor_twin_line] (radioctl-protocols) Aor twin line.
- [@ANCHOR: radioctl:radioctl_uniden_engine] (radioctl-protocols) The engine for a Uniden description.
- [@ANCHOR: radioctl:radioctl_uniden_dialect_from_description] (radioctl-protocols) Uniden dialect from description.
- [@ANCHOR: radioctl:radioctl_uniden_dialect_encode] (radioctl-protocols) Uniden dialect encode.
- [@ANCHOR: radioctl:radioctl_uniden_dialect_status] (radioctl-protocols) Uniden dialect status.
- [@ANCHOR: radioctl:radioctl_uniden_dialect_decode] (radioctl-protocols) Uniden dialect decode.
- [@ANCHOR: radioctl:radioctl_uniden_parse_units] (radioctl-protocols) A frequency from eight digits in units of 100 Hz.
- [@ANCHOR: radioctl:radioctl_uniden_mode_named] (radioctl-protocols) The mode a radio's name stands for (`NFM` reads as FM), if the description has it.
- [@ANCHOR: radioctl:radioctl_uniden_twin_new] (radioctl-protocols) The twin of a Uniden description.
- [@ANCHOR: radioctl:radioctl_uniden_twin_line] (radioctl-protocols) Uniden twin line.
- [@ANCHOR: radioctl:radioctl_conformance_receiver_engine] (radioctl-protocols) The engine of a receiver-family description, by its protocol.
- [@ANCHOR: radioctl:radioctl_jrc_engine] (radioctl-protocols) The engine for a JRC description.
- [@ANCHOR: radioctl:radioctl_jrc_dialect_from_description] (radioctl-protocols) Jrc dialect from description.
- [@ANCHOR: radioctl:radioctl_jrc_dialect_encode] (radioctl-protocols) Jrc dialect encode.
- [@ANCHOR: radioctl:radioctl_jrc_dialect_status] (radioctl-protocols) Jrc dialect status.
- [@ANCHOR: radioctl:radioctl_jrc_dialect_decode] (radioctl-protocols) Jrc dialect decode.
- [@ANCHOR: radioctl:radioctl_jrc_info_checked] (radioctl-protocols) The `I` answer if it has the length the model's answer has (the CR counts), else `None`.
- [@ANCHOR: radioctl:radioctl_jrc_info_frequency] (radioctl-protocols) The frequency in an `I` answer: from the fifth character on the NRD, after three
- [@ANCHOR: radioctl:radioctl_jrc_info_mode] (radioctl-protocols) The mode in an `I` answer: its fourth character.
- [@ANCHOR: radioctl:radioctl_jrc_twin_new] (radioctl-protocols) The twin of a JRC description.
- [@ANCHOR: radioctl:radioctl_jrc_twin_line] (radioctl-protocols) Jrc twin line.
- [@ANCHOR: radioctl:radioctl_jrc_twin_set] (radioctl-protocols) Applies a setting (`F...` or `D.`); anything else is ignored, as these radios do not refuse.
- [@ANCHOR: radioctl:radioctl_pcr_engine] (radioctl-protocols) The engine for an Icom PCR description.
- [@ANCHOR: radioctl:radioctl_pcr_tuning_command] (radioctl-protocols) The `K0` tuning command for a frequency, mode character and filter character.
- [@ANCHOR: radioctl:radioctl_pcr_dialect_from_description] (radioctl-protocols) Pcr dialect from description.
- [@ANCHOR: radioctl:radioctl_pcr_dialect_encode] (radioctl-protocols) Pcr dialect encode.
- [@ANCHOR: radioctl:radioctl_pcr_dialect_status] (radioctl-protocols) Pcr dialect status.
- [@ANCHOR: radioctl:radioctl_pcr_dialect_decode] (radioctl-protocols) Pcr dialect decode.
- [@ANCHOR: radioctl:radioctl_pcr_twin_new] (radioctl-protocols) The twin of an Icom PCR description.
- [@ANCHOR: radioctl:radioctl_pcr_twin_line] (radioctl-protocols) Pcr twin line.
- [@ANCHOR: radioctl:radioctl_marine_engine] (radioctl-protocols) The engine for an Icom marine description.
- [@ANCHOR: radioctl:radioctl_marine_sentence] (radioctl-protocols) The command sentence for `command` (with its `,param` if any) to radio `id`, checksum and CR LF.
- [@ANCHOR: radioctl:radioctl_marine_dialect_from_description] (radioctl-protocols) Marine dialect from description.
- [@ANCHOR: radioctl:radioctl_marine_dialect_encode] (radioctl-protocols) Marine dialect encode.
- [@ANCHOR: radioctl:radioctl_marine_dialect_more_lines] (radioctl-protocols) Marine dialect more lines.
- [@ANCHOR: radioctl:radioctl_marine_dialect_status] (radioctl-protocols) Marine dialect status.
- [@ANCHOR: radioctl:radioctl_marine_dialect_decode] (radioctl-protocols) Marine dialect decode.
- [@ANCHOR: radioctl:radioctl_marine_mode_of] (radioctl-protocols) The mode a `MODE` answer names, if the description has it.
- [@ANCHOR: radioctl:radioctl_marine_fields_parse] (radioctl-protocols) Splits a sentence, checking `$PICOA,`, the checksum and the minimum length Hamlib requires.
- [@ANCHOR: radioctl:radioctl_marine_twin_new] (radioctl-protocols) The twin of an Icom marine description.
- [@ANCHOR: radioctl:radioctl_marine_twin_line] (radioctl-protocols) Marine twin line.
- [@ANCHOR: radioctl:radioctl_rs_engine] (radioctl-protocols) The engine for a Rohde and Schwarz description.
- [@ANCHOR: radioctl:radioctl_rs_dialect_from_description] (radioctl-protocols) Rs dialect from description.
- [@ANCHOR: radioctl:radioctl_rs_dialect_encode] (radioctl-protocols) Rs dialect encode.
- [@ANCHOR: radioctl:radioctl_rs_dialect_status] (radioctl-protocols) Rs dialect status.
- [@ANCHOR: radioctl:radioctl_rs_dialect_decode] (radioctl-protocols) Rs dialect decode.
- [@ANCHOR: radioctl:radioctl_rs_twin_new] (radioctl-protocols) The twin of a Rohde and Schwarz description.
- [@ANCHOR: radioctl:radioctl_rs_twin_line] (radioctl-protocols) Rs twin line.
- [@ANCHOR: radioctl:radioctl_marine_mode_name] (radioctl-protocols) The name the radio uses for a mode, if the description has it.
- [@ANCHOR: radioctl:radioctl_pcr_dialect_terminator] (radioctl-protocols) Pcr dialect terminator.
- [@ANCHOR: radioctl:radioctl_receiver_dialect_status_default] (radioctl-protocols) Receiver dialect status default.
