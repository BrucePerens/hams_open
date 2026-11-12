# radioctl

A thread-safe, async-only Rust rig-control library intended to replace Hamlib in the relay, plus a
`rigctld`-protocol server (`radioctld`).

- Licence: AGPL-3.0-or-later (Bruce's decision of 2026-10-30). Every Rust file carries an SPDX header
  and the manifests inherit the licence from the workspace.
- Planning: `hams_com/docs/proposals/RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`.
- Programmer's manual: [`docs/manual/README.md`](docs/manual/README.md).
- Design decisions: [`docs/DESIGN.md`](docs/DESIGN.md).

## Status

- Phase 0/1 foundation. One radio, the Elecraft KX3, has been used on a bench (below); everything else
  has never been used with real hardware. Treat every item below as `unverified` unless it says otherwise.
- Implemented (tests on `main`, no radio):
  - Vocabulary, sans-IO protocol engines and virtual rigs.
  - Rig actor with queue, cache, polling, events and the transmit gate.
  - Real serial (USB-serial identified by vid/pid/serial, hot-plug reconnect, gated line keying) and TCP/UDP network links (reconnect with backoff).
  - USB SDR boards and dongles (Si570 kits, FiFi-SDR, FUNcube, Digital World Traveller) over a USB link: request/reply frames, a `UsbBackend` trait with a fake and a libusb backend (cargo feature `usb`, off by default), hot-plug reconnect and the same unkey-on-reopen rule. All unverified on hardware.
  - `radioctld` (rig and rotator servers), the protocol splitter, `radioctl-station` and a C ABI sketch.
- Protocol families: Icom CI-V, Kenwood ASCII (including the TS-890S dialect and the handheld/mobile family), Elecraft, Yaesu newcat and the legacy block protocols, FlexRadio SmartSDR, OpenHPSDR (control plane only), Xiegu (CI-V), the receiver families (AOR, Uniden, JRC, Icom PCR and marine, Rohde and Schwarz), about thirty small serial-vendor engines (Ten-Tec, Racal, Drake, Barrett, CODAN, MDS, MICOM, WJ, Paragon, XG3, Transfox and others), a UDP control frame (HiQSDR) and USB SDR boards.
- Rig descriptions: <!--count:description_files-->292<!--/count--> files in `rigs/`, <!--count:described-->283<!--/count--> of the <!--count:real_radios-->283<!--/count--> real radios among the <!--count:models-->313<!--/count--> models Hamlib 4.6.2 lists (<!--count:not_applicable-->30<!--/count--> are not radios or have no transport radioctl speaks). The numbers are marked in the text and `python3 tools/doc_counts.py --check` (and its test) fails when they drift from `docs/RIG_SUPPORT.md`, the generated list. <!--count:hardware_verified-->1<!--/count--> description is `hardware-verified`: the Elecraft KX3 (MCU, microcontroller, 3.02 / DSP, digital signal processor, 1.52, bench run with transmit into a dummy load, 2026-11-05); every fact the run exercised says what ran, and front-panel auto-information and the virtual-rig defaults stay unverified. Every other fact is `unverified` or `manual-checked` until a volunteer confirms it with `docs/VOLUNTEER_TESTING.md`.
- Transmit: only the KX3 bench run (a dummy load, the gate opened for it) has keyed real hardware. Anything that can transmit must fail closed behind an explicit capability gate.

## Getting started

- **Toolchain.** `rust-toolchain.toml` pins Rust 1.98.1 (with rustfmt and clippy); the workspace declares
  `rust-version = "1.98"`.
- **Build.** From `daemons/radioctl`: `cargo build --release -p radioctld` builds the two programs of that crate,
  `radioctld` (the `rigctld`-compatible server) and `radioctl-selftest` (the volunteer test tool). Cargo features
  (all off unless said): `usb` on `radioctld` and `radioctl-core` (direct libusb access for USB boards and
  dongles; needs libusb-1.0), `hamlib-oracle` on `radioctld` and `radioctl-protocols` (differential tests
  against Hamlib's own binaries), `owned-runtime` on `radioctl-core` (a runtime of the library's own, for
  non-tokio programs; `radioctl-capi` turns it on), and `tokio-io` on `radioctl-protocols` (on by default).
- **Try it with no radio.** `cargo run -p radioctld --example quickstart` drives a virtual KX3 in a dozen lines
  using the prelude (`use radioctl_protocols::prelude::*;`); the code is in chapter 2 of the manual and is
  the same file. `radioctld` serves a virtual radio from [`radioctld.example.toml`](radioctld.example.toml).
- **Test.** `cargo test --all-features --no-fail-fast` runs everything, including the manual's own check
  (`cargo test -p radioctld --test manual`); the Hamlib oracle needs Hamlib 4.6.2's `rigctld` and `rigctl`.
  The Python tools have tests of their own: `python3 -m pytest tools/`. Details: manual chapter 11.
- **Read next.** The [programmer's manual](docs/manual/README.md); the generated
  [rig support list](docs/RIG_SUPPORT.md); the [volunteer testing guide](docs/VOLUNTEER_TESTING.md) (run
  `radioctl-selftest` on your radio, read-only); the [recommended hardware](docs/RECOMMENDED_HARDWARE.md); the
  [bench runbook](docs/BENCH_VERIFICATION.md) for the maintainers' hardware checks.
- **Licence.** AGPL-3.0-or-later; the SPDX header is on every Rust file.

## Layout

```
daemons/radioctl/
  Cargo.toml              workspace (resolver 2), shared lints: no unsafe, no unwrap/expect/panic
  rust-toolchain.toml     pinned toolchain
  radioctld.example.toml  annotated example configuration
  crates/radioctl-core/       typed values, capability model, errors, rig actor, network and USB links (`usb_device` traits and fake bus, `usb_wire` frames, `usb_link`, `usb_libusb` behind the `usb` feature), rig-file loader
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
  tools/doc_counts.py           keeps the marked numbers in this README and the manual equal to docs/RIG_SUPPORT.md and rigs/ (--check fails on drift)
  tools/wire_to_transcript.py   turns a bench_console --wire capture and its --log into a transcript fixture
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
  - The TOML (a plain-text configuration format) format (`rigs/*.toml`, below) deserialises into the same struct.
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
| Yaesu legacy five-byte | `yaesu-ft-817`, `yaesu-ft-857d`, `yaesu-ft-897d`, `yaesu-ft-897`, `yaesu-ft-847`, `yaesu-ft-847uni`, `yaesu-ft-650`, `yaesu-ft-736r`, `yaesu-ft-818`, and the block-protocol models `yaesu-ft-747gx`, `yaesu-ft-757gx`, `yaesu-ft-757gxii`, `yaesu-ft-767gx`, `yaesu-ft-840`, `yaesu-ft-890`, `yaesu-ft-900`, `yaesu-ft-920`, `yaesu-ft-990`, `yaesu-ft-990-oldrom`, `yaesu-ft-1000`, `yaesu-ft-1000d`, `yaesu-ft-1000mp`, `yaesu-ft-1000mp-mkv`, `yaesu-ft-1000mp-mkv-field`, `yaesu-ft-100`, `yaesu-ft-600`, `yaesu-ft-980`, `yaesu-frg-100`, `yaesu-frg-8800`, `yaesu-frg-9600`, `yaesu-vr-5000`, `vertex-vx-1700` | Operating manuals' CAT chapters The FT-897, FT-847, FT-847UNI, FT-650 and FT-736R files are from Hamlib 4.6.2 facts only (the FT-897 shares the FT-897D's CAT chapter); the last three are set-only. The block-protocol models (opcode 10 status reads, per-model frequency byte order and units, all described in the files as data) come from the Yaesu manuals' CAT chapters where a copy was found (FT-747GX, FT-757GXII, FT-767GX, FT-840, FT-890, FT-900, FT-920, FT-990, FT-1000, FT-1000MP MARK-V, FT-100, FRG-100, FRG-8800, VR-5000) and from Hamlib facts for the FT-600, FT-980, FRG-9600 and VX-1700; every frequency unit or offset the two sources disagree on is an open question pinned in tests. |
| AOR receivers (ASCII `RF`/`MD`/`LM`) | `aor-ar8000`, `aor-ar8200`, `aor-ar8600`, `aor-ar5000`, `aor-ar5000a`, `aor-ar2700`, `aor-sr2200` | Hamlib 4.6.2 source (facts only) |
| AOR AR3030 and AR3000A (ASCII `D`/`Y`, MHz entry) | `aor-ar3030`, `aor-ar3000a` | AOR operating manuals' RS-232 chapters (manual-checked) |
| AOR AR7030 and AR7030 Plus (binary nibble protocol) | `aor-ar7030`, `aor-ar7030p` | AOR's remote protocol text (manual-checked; the Plus by Hamlib's reading) |
| Uniden scanners (ASCII `RF`/`RM`/`SG`) | `uniden-bc245xlt`, `uniden-bc250d`, `uniden-bc780xlt`, `uniden-bc895xlt`, `uniden-bc898t` | Hamlib 4.6.2 source (facts only) |
| Uniden digital scanners (`GLG`/`QSH`) | `uniden-bcd396t`, `uniden-bcd996t` | Uniden's remote command specification (facts only; manual-checked) |
| JRC (ASCII `F`/`D`/`I`) | `jrc-nrd-525`, `jrc-nrd-535d`, `jrc-nrd-545`, `jrc-jst-145`, `jrc-jst-245` | Hamlib 4.6.2 source (facts only) |
| Icom PCR (ASCII `K0`) | `icom-ic-pcr100`, `icom-ic-pcr1000`, `icom-ic-pcr1500`, `icom-ic-pcr2500` | Hamlib 4.6.2 source (facts only) |
| Icom marine (`$PICOA`) | `icom-ic-m700pro`, `icom-ic-m710`, `icom-ic-m802`, `icom-ic-m803` | Hamlib 4.6.2 source (facts only) |
| Rohde and Schwarz (ASCII `FREQ`/`DEM`) | `rs-esmc`, `rs-eb200` | Hamlib 4.6.2 source (facts only) |
| Rohde and Schwarz HF (LF/CR blocks `F`/`I`, `*F`/`*I`) | `rs-ek895`, `rs-ek896` (operator manual, manual-checked), `rs-xk2100`, `rs-xk852` (Hamlib 4.6.2 source) | EK 895/896 manual chapter 2; Hamlib facts only |
| OptoScan boards (CI-V at 0x80) | `optoscan-os535`, `optoscan-os456` | Hamlib 4.6.2 source (facts only) |
| Yaesu legacy five-byte | `yaesu-ft-817`, `yaesu-ft-857d`, `yaesu-ft-897d`, `yaesu-ft-818` | Operating manuals' CAT chapters |
| FlexRadio | `flex-6300`, `flex-smartsdr-slice-a`, `flex-smartsdr-slice-b`, `flex-smartsdr-slice-c`, `flex-smartsdr-slice-d`, `flex-smartsdr-slice-e`, `flex-smartsdr-slice-f`, `flex-smartsdr-slice-g`, `flex-smartsdr-slice-h`, `flex-6400`, `flex-6600`, `flex-6700` | FlexRadio's SmartSDR API wiki |
| OpenHPSDR | `anan-10`, `anan-100` | `daemons/hpsdr` and the OpenHPSDR protocols |

### Icom CI-V

- The first four Icom files (IC-7300, 705, 7610, 9700) are **not written from Icom's guides**. For the 19 later models Icom's instruction manuals were read for facts, under Bruce Perens' decision of 2026-10-31 (the terms are unreasonable and would not survive a court): Icom's download endpoint is excluded by robots.txt and the direct path is 403, so the archive.org mirror copies were read; they are kept outside every repository (`~/manuals/icom/`, sha256 in `SHA256SUMS`) and are not redistributed.
- Their facts were read from Hamlib 4.6.2 source (facts only, provenance kind `hamlib-source`, file and function named per fact) and from `rigctl --dump-caps` output.
- Engine features: VFO B, split, RIT/XIT (receiver and transmitter incremental tuning, small frequency offsets), levels, filter, model id.
- Raw meters (`Op::GetMeter`: SWR (standing wave ratio), ALC (automatic level control), COMP (speech compression), Vd and Id (power-amplifier drain voltage and current) as 0 to 255 deflections, uncalibrated) and data modes (`USB-D`, `LSB-D`; `M PKTUSB` / `PKTLSB` through `radioctld`) are implemented for the IC-7300. `radioctld` serves them as Hamlib's `ALC`, `SWR`, `COMP_METER`, `VD_METER` and `ID_METER` through unverified scale points (manual chapters 9 and C). The engine takes the mode being set as the mode for the filter table.
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
  - the bandscope (`BS0`-`BS9`, `BSC`, `BSM`, `DD0`, LAN `##DD2` and `##DD3` frames) is implemented against the virtual rig only;
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

- Files: `flex-6300`, `flex-smartsdr-slice-a`, `flex-smartsdr-slice-b`, `flex-smartsdr-slice-c`, `flex-smartsdr-slice-d`, `flex-smartsdr-slice-e`, `flex-smartsdr-slice-f`, `flex-smartsdr-slice-g`, `flex-smartsdr-slice-h`, `flex-6400`, `flex-6600`, `flex-6700`; family `flex-smartsdr`; a `[flex]` block.
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

### radiod receive channels (RX-888 and other ka9q-radio front ends)

- Rig: one receive channel of a ka9q-radio `radiod`; family `radiod-channel`, no family block; file `radiod-channel`. Each channel is a virtual rig, so one RX-888 can serve many of them, one per SSRC.
- **Receive only.** The file claims `read-frequency`, `set-frequency`, `read-mode`, `set-mode`, `filter` and `read-signal`. The engine refuses any `radiod-channel` description that claims a transmit capability, and it encodes no transmit operation. radioctld therefore reports no PTT (`ptt_type=0x0`, "Can set PTT: N"), even with `transmit = true`, and a console reading those capabilities hides Transmit. `radiod` itself has no transmit path.
- Structure, as for OpenHPSDR: a line-protocol engine (`radiod_net.rs`) over an in-memory stream to a bridge (`radiod_io.rs`). The bridge owns the `radiod_client` sockets (`daemons/radiod_client`, a closed command schema in which every channel command carries a lifetime) and the channel's RTP audio.
- Configuration (radioctld):

  ```toml
  [[rig]]
  name = "rx888"
  description = "rigs/radiod-channel.toml"
  [rig.transport]
  kind = "radiod"
  status = "239.88.8.1"     # radiod's status group (a literal IPv4 multicast address)
  data = "239.88.8.2"       # its data (RTP) group
  ssrc = 7001               # this channel; unique among the clients of this radiod
  # optional: status_port = 5006, data_port = 5004, iface = "127.0.0.1",
  #           lifetime_frames = 500, refresh_ms = 2000, min_hz = 10000, max_hz = 30000000
  ```

- Mode changes bring back the mode's own passband. `SetFilterWidth` places the passband per mode: USB and LSB keep their 50 Hz edge next to the carrier, CW centres on its 600 Hz tone, AM and FM centre on the carrier.
- Signal (`RAWSTR`): the channel's baseband power in tenths of a dB above -200 dB re full scale. It is referred to the input (`radiod` removes the front end's RF gain), not calibrated to dBm, and not an S-meter.
- The channel is kept by re-asserting its whole state every `refresh_ms`, never by a bare poll. So a `radiod` that restarted gets the channel back at the right frequency (seen on the RX-888: one missed acknowledgement, then the channel was back at 14.074 MHz). When the rig is dropped the channel is removed. If the process dies, the channel expires by its lifetime.
- Audio is `RadiodControl::take_audio` (16-bit PCM decoded to mono `f32`, 12 kHz, 24 kHz for FM), not a radioctl operation. `RadiodControl::front_end` gives the front end's overrange count and RF gain.
- **Verified** against an RX-888 MkII on `radiod` 8c50dbbe, loopback, with no antenna (2026-11-05): tuning, mode, width, signal, PTT refused, audio at 50 frames/s with 0 lost, and the restart recovery above (`hams_com/docs/reviews/RX888_BRINGUP_2026-11-05.md`). Not yet verified: a real signal (no antenna yet) and the relay wiring.

#### radiod channel anchor reference (ADR 0113)

Files: `crates/radioctl-protocols/src/radiod_net.rs` (engine, pure, no I/O), `crates/radioctl-protocols/src/radiod_io.rs` (bridge; feature `tokio-io`, which is on by default), `crates/radioctl-protocols/src/conformance.rs` (stand-in), `crates/radioctld/src/config.rs` and `rigs.rs` (configuration). Everything in `radiod_net.rs` is synchronous and pure. The bridge runs two tasks on the caller's tokio runtime. Its shared state is behind `std::sync::Mutex`es that are held only for a copy or an update, never across an `.await`. Nothing here can key a transmitter.

##### Data structures

###### `RadiodLink` and `RadiodLink::new` [@ANCHOR: radioctl:radioctl_radiod_link_new]

`RadiodLink::new(status_group: Ipv4Addr, data_group: Ipv4Addr, ssrc: u32) -> RadiodLink`. Where a channel lives and how it is kept: the defaults below, adjusted field by field before `serve_radiod`. Owned by the caller and moved into the bridge.

| Field | Type, unit | Default | Meaning |
|---|---|---|---|
| `status_group`, `status_port` | `Ipv4Addr`, `u16` | given, 5006 | `radiod`'s status (control) group and port |
| `data_group`, `data_port` | `Ipv4Addr`, `u16` | given, 5004 | its data (RTP) group and port |
| `iface` | `Ipv4Addr` | 127.0.0.1 | interface to join on; the client always sends with TTL 0 |
| `ssrc` | `u32` | given | the channel; 1 to 0xfffffffe, unique among this `radiod`'s clients (two clients on one SSRC share one channel) |
| `lifetime_frames` | `u32`, 20 ms frames | 500 (10 s) | sent with every command; 1 to `policy.max_lifetime_frames` |
| `refresh` | `Duration` | 2 s | keep-alive interval; non-zero, at most a third of the lifetime |
| `ack_wait` | `Duration` | 1 s | wait for one acknowledgement |
| `policy` | `radiod_client::command::Policy` | default (10 kHz to 30 MHz) | frequencies outside are refused before sending |
| `frequency_hz` | `u64`, Hz | 10 000 000 | starting dial frequency |
| `mode` | `Mode` | AM | starting mode, USB/LSB/CW/AM/FM/SAM/SAM-L/SAM-U |
| `audio_queue` | `usize`, frames | 50 (1 s) | audio frames held before new ones are dropped |

###### `ModeSpec` [@ANCHOR: radioctl:radioctl_radiod_mode_spec]

`Copy` value. `name: &'static str` is the line-protocol name (`usb`, `lsb`, `cw`, `am`, `fm`, `sam`, `sam-l`, `sam-u`). `default_width_hz: u64` is the width a mode change brings back: USB and LSB 2950, CW 400, AM and SAM 10 000, FM 16 000, SAM-L and SAM-U 5 100 (the `radiod_client` modes' own passbands: 5 kHz of sideband plus 100 Hz across the carrier).

`AudioFrame` (fields `samples: Vec<f32>` mono -1..=1, `rate_hz: u32`, `seq: u16`, `timestamp: u32`) and `RadiodStats` (counters `acks`, `ack_timeouts`, `refused`, `audio_frames`, `audio_dropped`, `rtp_lost`, `rtp_out_of_order`, `rtp_restarts`, `rtp_stalls`, `rtp_stall_ms`, `rtp_bad`, plus the latest `channel` and `front_end` statuses) are plain data returned by the control handle, documented with `take_audio` and `stats` below.

##### Engine functions (`radiod_net.rs`)

**`radioctl_radiod_signal_raw`** [@ANCHOR: radioctl:radioctl_radiod_signal_raw] `signal_raw(db: f32) -> u16`. Converts a baseband power in dB re full scale (the `radiod` channel status value) to the unsigned raw signal the `GS`/`S` line carries and a radioctl `Signal` level reads: `round((db + 200) * 10)` (the floor `SIGNAL_FLOOR_DB` is -200 dB, resolution 0.1 dB), clamped to `0..=65535`, so anything below -200 dB is 0 and anything above 6 353.5 dB is 65535. A non-finite input (NaN or infinity, for a channel that has not reported power) gives 0. Pure, no errors, no state; the inverse is `signal_db`. Called by `Bridge::handle_line` when it answers `GS`.

**`radioctl_radiod_signal_db`** [@ANCHOR: radioctl:radioctl_radiod_signal_db] `signal_db(raw: u16) -> f32`. The inverse of `signal_raw`: `-200 + raw / 10` dB re full scale, an exact tenth of a dB for any `raw` in `0..=65535` (0 is -200.0 dB, 2000 is 0 dB). The value is referred to the input, not calibrated to dBm (see the RAWSTR note above). Pure and total; used by the tests and by callers that want the dB figure back from the stored raw level.

**`radioctl_radiod_mode_name`** [@ANCHOR: radioctl:radioctl_radiod_mode_name] `mode_name(mode: Mode) -> Option<&'static str>`. The line-protocol name of USB, LSB, CW, AM, FM, SAM, SAM-L or SAM-U (`usb`, `lsb`, `cw`, `am`, `fm`, `sam`, `sam-l`, `sam-u`: the rig-file names of `radioctl_core::units::Mode`), otherwise `None` (the mode is refused). Pure.

**`radioctl_radiod_mode_from_name`** [@ANCHOR: radioctl:radioctl_radiod_mode_from_name] `mode_from_name(name: &str) -> Option<Mode>`. The inverse of `mode_name`: `usb`, `lsb`, `cw`, `am`, `fm`, `sam`, `sam-l` and `sam-u` give the matching `Mode`; anything else gives `None`, including upper-case or padded text (the match is exact and case-sensitive) and the names of modes a `radiod` channel cannot demodulate (for example `rtty`). Borrows `name`; pure. `parse_line` uses it to read the `M <name>` line the bridge answers.

**`radioctl_radiod_mode_spec_of`** [@ANCHOR: radioctl:radioctl_radiod_mode_spec_of] `ModeSpec::of(mode: Mode) -> ModeSpec`. Returns the `Copy` spec (line-protocol `name` and `default_width_hz`) of a mode, the single place the per-mode defaults are kept. A mode `radiod` cannot demodulate gets USB's spec rather than an error or a panic, because callers have already refused such a mode through `mode_name` (which returns `None`) before they ask for a spec; relying on the fallback would give a wrong passband. Pure and total.

**`radioctl_radiod_mode_spec_edges`** [@ANCHOR: radioctl:radioctl_radiod_mode_spec_edges] `ModeSpec::edges(self, width_hz: u64) -> (f32, f32)`. Passband edges in Hz from the carrier for a width clamped to 1..=24 000 Hz. USB is `(50, 50+w)`, LSB `(-50-w, -50)`, CW `(600-w/2, 600+w/2)`, AM, SAM and FM `(-w/2, w/2)`, SAM-L `(100-w', 100)` and SAM-U `(-100, w'-100)` where `w' = max(w, 200)` (`SAM_CARRIER_GUARD_HZ`, `SAM_SIDEBAND_MIN_WIDTH_HZ`: the PLL needs the carrier inside the passband, never at an edge). Each edge is clamped to ±`MAX_EDGE_HZ` (12 kHz, equal to `radiod_client`'s limit and pinned by a test).

**`radioctl_radiod_check_receive_only`** [@ANCHOR: radioctl:radioctl_radiod_check_receive_only] `check_receive_only(desc: &RigDescription) -> Result<(), RigError>`. The transmit guard of the `radiod` engine: it scans the description's `capabilities` for the first one whose `transmits()` is true (`set-ptt`, `send-cw`) and returns `Err` of kind `Unsupported` with the message "a radiod channel is receive-only; the description claims <capability>"; with none it returns `Ok(())`. A `radiod` channel is demodulation only, and a description that claimed transmit would make a console offer a Transmit button, so the refusal happens when the engine is built, before any link exists. Borrows the description; pure.

**`radioctl_radiod_engine_new`** [@ANCHOR: radioctl:radioctl_radiod_engine_new] `RadiodEngine::new(desc: RigDescription) -> Result<RadiodEngine, RigError>`. Builds the sans-IO engine for one channel from an owned `radiod-channel` description (`Family::Radiod`). It returns `Err` of kind `Unsupported` ("not a radiod-channel description") for another family, and the error of `check_receive_only` for a description that claims a transmit capability. On success the engine holds the description, an empty line buffer and no discard state. No I/O, no locks, nothing is sent; `build_rig` calls it before `serve_radiod`.

**`radioctl_radiod_engine_encode`** [@ANCHOR: radioctl:radioctl_radiod_engine_encode] `ProtocolEngine::encode(&mut self, op: &Op) -> Result<Request, RigError>`. Encodes `GetFrequency(A)` as `GF`, `SetFrequency(A, hz <= u32::MAX)` as `F <hz>`, `GetMode` as `GM`, `SetMode` of USB/LSB/CW/AM/FM/SAM/SAM-L/SAM-U as `M <name>`, `GetFilterWidth` as `GW`, `SetFilterWidth(1..=24000)` as `W <hz>`, and `GetSignal` as `GS`, each with a newline. Sets expect `Ack`, reads expect the field. Anything else, PTT and CW included, is `Err(Unsupported)`.

**`radioctl_radiod_engine_feed`** [@ANCHOR: radioctl:radioctl_radiod_engine_feed] `ProtocolEngine::feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>`. Splits bytes into lines (lines may span calls) and parses each with `parse_line`. A line longer than 128 bytes yields one `Unrecognised(empty)` and is discarded up to its newline.

**`radioctl_radiod_engine_parse_line`** [@ANCHOR: radioctl:radioctl_radiod_engine_parse_line] `parse_line(line: &str) -> Inbound` (private). Maps `OK` to `Ack`, `ERR ...` to `Nak`, `F <hz>` to a frequency, `M <name>` to a mode, `W <hz>` to a filter width, and `S <raw <= 65535>` to a signal. Anything else is `Unrecognised`.

**`radioctl_radiod_engine_link_restored`** [@ANCHOR: radioctl:radioctl_radiod_engine_link_restored] `ProtocolEngine::link_restored(&mut self) -> Vec<Request>`. Called by the rig actor when the link comes back after a loss. It clears the engine's partial-line buffer and the discard-oversize flag, so a half line from before the loss is not glued to the first bytes after it, and returns an empty request list: unlike a serial rig there is nothing to re-send, because the bridge (`run_control`) re-asserts the whole channel state on every refresh. Cannot fail; no I/O.

##### Bridge functions (`radiod_io.rs`)

**`radioctl_radiod_link_validate`** [@ANCHOR: radioctl:radioctl_radiod_link_validate] `RadiodLink::validate(&self) -> Result<(), RigError>`. `Err(Unsupported)` naming the field when any of these fails: the SSRC is usable, the starting mode is supported, the lifetime is in 1..=policy maximum, the refresh is non-zero and at most a third of the lifetime, and `ack_wait` and `audio_queue` are non-zero.

**`radioctl_radiod_serve`** [@ANCHOR: radioctl:radioctl_radiod_serve] `serve_radiod(runtime: &Handle, link: RadiodLink) -> Result<(DuplexStream, RadiodControl), RigError>`. Validates the link and opens the control and RTP sockets. It returns `Err(LinkDown)` if a socket cannot be opened, for example a group that is not multicast or no multicast on the interface. It then spawns the control task and the audio task on `runtime`, and returns the actor's end of the stream with the control handle. Synchronous; the channel is created by the control task straight after.

**`radioctl_radiod_control_take_audio`** [@ANCHOR: radioctl:radioctl_radiod_control_take_audio] `RadiodControl::take_audio(&self) -> Option<mpsc::Receiver<AudioFrame>>`. Hands over the receiver of the channel's audio queue (capacity `RadiodLink::audio_queue` frames, 50 by default, one second) to the one consumer. The first call returns `Some`, every later call (on this handle or any clone) returns `None`, because the receiver moves out of the shared state. Until it is taken, and whenever the consumer does not read fast enough, new frames are dropped and counted in `audio_dropped`, never buffered without bound and never blocking the audio task. Takes a short mutex hold; safe from any thread; does not touch `radiod`.

**`radioctl_radiod_control_stats`** [@ANCHOR: radioctl:radioctl_radiod_control_stats] `RadiodControl::stats(&self) -> RadiodStats`. Returns an owned copy of `RadiodStats` taken under one short mutex hold: the counters `acks`, `ack_timeouts`, `refused`, `audio_frames`, `audio_dropped`, `rtp_lost`, `rtp_out_of_order`, `rtp_restarts`, `rtp_stalls`, `rtp_stall_ms` and `rtp_bad`, plus the latest status of the channel and of the front end (each `None` until `radiod` has answered). The copy is a consistent snapshot, not a live view. Non-blocking, cannot fail, no effect on `radiod`; for diagnostics and the front-end overrange display.

**`radioctl_radiod_control_front_end`** [@ANCHOR: radioctl:radioctl_radiod_control_front_end] `RadiodControl::front_end(&self) -> Option<ChannelStatus>`. A clone of the latest status of `radiod`'s front end (SSRC 0), with fields such as `ad_over` (ADC overrange count), `rf_gain_db`, `rf_atten_db` and `input_samprate`. The bridge reads it at every keep-alive refresh (2 s by default), so it is at most one refresh old; `None` until `radiod` has answered the first front-end read, and a failed read leaves the previous value in place. Short mutex hold, non-blocking, receive only.

**`radioctl_radiod_control_carrier_lock`** [@ANCHOR: radioctl:radioctl_radiod_control_carrier_lock] `RadiodControl::carrier_lock(&self) -> Option<bool>`. The carrier lock of the channel's synchronous detector (`radiod`'s PLL), for a console's Sync indicator (hams_com reports it as `sync_locked` in `get_rig_state`). Borrows the handle and returns a tri-state `Option<bool>` described in the table below; it cannot fail.

| Returns | When |
|---|---|
| `Some(true)` | the mode the engine last set successfully is SAM, SAM-L or SAM-U, and the channel's latest status says the PLL is on and locked (`radiod_client::status::ChannelStatus::carrier_lock`) |
| `Some(false)` | as above, but the status says the PLL is not locked |
| `None` | any other mode (no PLL), no status yet, or a status that does not report the PLL |

Synchronous, non-blocking apart from two short mutex holds (the mode, then the counters; never both at once), safe from any thread. Sends nothing: the lock is as fresh as the latest acknowledged status, which arrives at every keep-alive refresh (`RadiodLink::refresh`, 2 s by default), every mode, frequency or width change, and every signal read (`GS` polls the channel). Receive only; no effect on `radiod`. Test: `synchronous_am_turns_radiods_pll_on_and_reports_its_lock` (fake `radiod`, lock toggled between polls).

**`radioctl_radiod_sam_passband`** [@ANCHOR: radioctl:radioctl_radiod_sam_passband] Constants in `radiod_net.rs`: `SAM_CARRIER_GUARD_HZ: f32 = 100.0`, how far a SAM-L or SAM-U passband reaches across the carrier (the value `radiod_client`'s `SamLower`/`SamUpper` and `radiod`'s `[ame]` preset use), and `SAM_SIDEBAND_MIN_WIDTH_HZ: f32 = 200.0`, the narrowest SAM-L or SAM-U passband `ModeSpec::edges` gives, so the carrier is never at an edge. Compile-time, no state.

**`radioctl_radiod_lock`** [@ANCHOR: radioctl:radioctl_radiod_lock] `lock(m: &Mutex<T>) -> MutexGuard<T>` (private). The one way the bridge takes any of its `std::sync::Mutex`es (stats, mode, format, audio queue): it locks and, if a previous holder panicked and poisoned the mutex, takes the data anyway instead of propagating the panic. That is safe here because every protected value is a counter, a status copy or a small plain value, never a half-updated invariant. The lock is held only for a copy or an update and never across an `.await`.

**`radioctl_radiod_radiod_mode`** [@ANCHOR: radioctl:radioctl_radiod_radiod_mode] `radiod_mode(mode: Mode) -> Option<radiod_client::command::Mode>` (private). The client crate's mode for USB/LSB/CW/AM/FM, and `Sam`, `SamLower`, `SamUpper` for SAM, SAM-L, SAM-U (radiod's carrier PLL on). `None` for anything else. Pure.

**`radioctl_radiod_state_commands`** [@ANCHOR: radioctl:radioctl_radiod_state_commands] `State::commands(&self, lifetime: u32) -> Vec<Command>` (private). Builds, from the bridge's desired channel state, the ordered command list that makes `radiod`'s channel match it: first an `Audio` command (frequency, mode through `radiod_mode`, 16-bit PCM output, and the given `lifetime` in 20 ms frames), then, only when a filter width has been set, a `Filter` command with the low and high edges from `ModeSpec::edges`. Pure; no I/O. `Bridge::assert` sends the list in order, and sending `Audio` for an SSRC that does not exist creates the channel.

**`radioctl_radiod_bridge_send`** [@ANCHOR: radioctl:radioctl_radiod_bridge_send] `async Bridge::send(&mut self, cmd: &Command) -> Result<ChannelStatus, String>` (private). Sends one command and waits up to `ack_wait` for its tag. It records the status and the stream format, and counts `acks`, `ack_timeouts` and `refused`. `Err` is the line-protocol error text.

**`radioctl_radiod_bridge_assert`** [@ANCHOR: radioctl:radioctl_radiod_bridge_assert] `async Bridge::assert(&mut self) -> Result<ChannelStatus, String>` (private). Makes `radiod`'s channel match the desired state by sending `State::commands` one at a time through `Bridge::send`, each waiting for its acknowledgement, and stops at the first failure (the error text of that command). On success it returns the status of the last command. If `radiod` has no such channel the first `Audio` command creates it, which is how a restarted `radiod` gets the channel back. Used by the keep-alive and by every set request; it only ever configures a receiver.

**`radioctl_radiod_bridge_refresh`** [@ANCHOR: radioctl:radioctl_radiod_bridge_refresh] `async Bridge::refresh(&mut self)` (private). The periodic keep-alive, run every `RadiodLink::refresh` (2 s by default, at most a third of the lifetime so one lost refresh does not let the channel lapse): it calls `assert` and ignores its result, then reads the front end with `ack_wait` as the limit and stores a successful answer in the shared stats. A failure is counted by `send` (`ack_timeouts`, `refused`) and is not reported to the actor; the next request from the actor reports it. No return value.

**`radioctl_radiod_bridge_handle_line`** [@ANCHOR: radioctl:radioctl_radiod_bridge_handle_line] `async Bridge::handle_line(&mut self, line: &str) -> String` (private). Answers one request:
- `GF`, `GM` and `GW` come from the latest status or the state.
- `GS` sends a lifetime-carrying poll and answers `S <signal_raw(baseband)>`.
- `F`, `M` and `W` update the state and `assert`. On failure they restore the old state and answer `ERR <why>`.
- Anything else answers `ERR command`.

**`radioctl_radiod_format_of`** [@ANCHOR: radioctl:radioctl_radiod_format_of] `format_of(s: &ChannelStatus) -> Option<Format>` (private). Reads the stream format a channel status announces: RTP payload type, encoding, channel count (1 or 2) and sample rate in Hz, and returns `Some(Format)` only when all four are present, the encoding is known and is not Opus (a PCM encoding), the payload type fits a byte, the channel count is 1 or 2 and the rate is above zero; otherwise `None`. `Bridge::send` stores it in the shared state, and `run_audio` keeps only packets whose payload type matches it. Pure.

**`radioctl_radiod_run_control`** [@ANCHOR: radioctl:radioctl_radiod_run_control] `async run_control(bridge, far, stop)` (private task, spawned on the caller's runtime by `serve_radiod`; `bridge` is owned, `far` is the bridge's end of the duplex stream, `stop` is the watch sender that tells the audio task to end). It first asserts the channel (creating it on `radiod`), then in one `select!` loop reads the actor's request lines from `far` (at most 128 bytes each; a longer line is discarded to its newline), answers each through `Bridge::handle_line`, and runs `Bridge::refresh` on every `refresh` tick (missed ticks are delayed, not bursted). The loop ends when the actor closes the stream or a write to it fails; the task then signals `stop` and sends `Remove` for the SSRC (lifetime 1, so the channel goes at once) and waits at most `ack_wait` for the answer. No return value; errors are counted in the stats or end the task. It only configures a receiver and never transmits.

**`radioctl_radiod_run_audio`** [@ANCHOR: radioctl:radioctl_radiod_run_audio] `async run_audio(rtp, ssrc, shared, tx, stop)` (private task). Reads the data group and keeps packets of `ssrc` whose payload type matches the announced format; others of that SSRC count as `rtp_bad`. It decodes to mono, tracks loss and offers each frame to the queue, until stopped. The sequence counters (`radiod_client`'s `SeqTracker`) are copied into `rtp_lost`, `rtp_out_of_order` and `rtp_restarts` after every packet: a channel `radiod` re-created comes back with its sequence a few hundred lower, which is one restart, not a run of late packets (until 2026-11-05 the whole new stream counted as out of order and the outage showed nowhere). Each packet's arrival time feeds `stall`.

**`radioctl_radiod_stall`** [@ANCHOR: radioctl:radioctl_radiod_stall] `stall(previous: Option<Instant>, now: Instant) -> Option<Duration>` (private). The gap since the channel's previous packet when it is at least `STALL_GAP` (250 ms; `radiod` sends a packet every 20 ms), else `None`, and `None` for the first packet. `run_audio` adds one to `rtp_stalls` and the gap to `rtp_stall_ms` for each, so an outage shows even when the sequence does not jump (a squelch-closed channel sends nothing, so its closures count too).

**`radioctl_conformance_radiod_stand_in_feed`** [@ANCHOR: radioctl:radioctl_conformance_radiod_stand_in_feed] `RadiodStandIn::feed(&mut self, bytes: &[u8]) -> Vec<u8>` (conformance, private). Answers the line protocol as the bridge does, with no `radiod`: frequency 10 kHz to 30 MHz, mode with its default width, width 1 to 24 000, and the signal fixed at `S 729`. Used by the shipped transcript `rigs/transcripts/radiod-channel-basic.toml`.

##### radioctld

**`radioctld_config_radiod_transport` / `RadiodTransport`** [@ANCHOR: radioctl:radioctld_config_radiod_transport] The checked `[rig.transport] kind = "radiod"`. Fields: `status_group`/`status_port` (`status`, `status_port` 1 to 65535, default 5006), `data_group`/`data_port` (`data`, `data_port`, default 5004), `iface` (an IPv4 address, default 127.0.0.1), `ssrc` (1 to 0xfffffffe), `lifetime_frames` (50 to 15 000, default 500), `refresh` (`refresh_ms` 200 to 60 000, default 2000, at most a third of the lifetime), and `min_hz`/`max_hz` (1 to 6 GHz, min below max, defaults 10 kHz and 30 MHz). Unknown keys are refused, so there is no TTL or destination key. Compared by value (`Eq`).

**`radioctld_config_multicast_group`** [@ANCHOR: radioctl:radioctld_config_multicast_group] `multicast_group(key: &str, text: &str) -> Result<Ipv4Addr, String>` (private). Parses the value of a configuration key (`status`, `data`) as a literal IPv4 multicast address and returns it, or `Err` with a sentence naming `key` when the text is not an IPv4 address or is not in the multicast range 224.0.0.0 to 239.255.255.255. A host name such as `hf.local` is refused, because there is no DNS or mDNS lookup. It runs when `radioctld` loads its configuration, so a bad group fails at startup.

**`radioctld_rigs_radiod_link`** [@ANCHOR: radioctl:radioctld_rigs_radiod_link] `radiod_link(t: &RadiodTransport, description: &RigDescription) -> RadiodLink`. The bridge settings: the transport's groups, ports, interface, SSRC, lifetime, refresh and frequency limits, with the description's starting frequency and mode. `build_rig` passes the result to `serve_radiod`, keeps the `RadiodControl` in `BuiltRig::radiod`, and refuses a `radiod` transport for any family other than `radiod-channel`.

**`radioctld_rigs_radiod_channel_description`** [@ANCHOR: radioctl:radioctld_rigs_radiod_channel_description] `radiod_channel_description() -> Result<RigDescription, String>`. The shipped `rigs/radiod-channel.toml`, compiled into the library and parsed exactly as `load_description` parses the file, so a program that embeds radioctld (the hams.com relay) can serve a `radiod` channel with no rigs directory. It is receive only: no capability it grants transmits. An error names the built-in description.

**`Served::radiod`** (in `radioctld_daemon_start_one`) `Option<RadiodControl>`. For a rig on a `radiod` transport, `start_one` keeps the bridge's control that `build_rig` returned in `BuiltRig::radiod`, so the program that started the daemon can take the channel's audio (`take_audio`, once) and read its counters and front-end status. `None` for every other transport. Before this the control was dropped at start, and the audio could not be reached through `daemon::start`.

## Rig coverage and volunteer testing

- radioctl aims to describe every Hamlib model (313 in 4.6.2) and to say honestly which ones a person has verified on a radio. The generated list is [docs/RIG_SUPPORT.md](docs/RIG_SUPPORT.md) (`python3 tools/rig_support_matrix.py`; a test fails when it is stale). Today one is hardware-verified: the Elecraft KX3 (2026-11-05).
- Which relay, switch and GPIO products suit the station layer, and which do not (the SainSmart 16-channel USB module is not recommended): [docs/RECOMMENDED_HARDWARE.md](docs/RECOMMENDED_HARDWARE.md). Provisional; nothing on it is hardware-verified.
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
- Hamlib 4.6.2 lists <!--count:models-->313<!--/count--> models (the proposal said 314; that count included the header line).
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
- USB boards (`kind = "usb"`): the description gives the ids and strings; optional `serial`, `vid`, `pid`, `transfer_timeout_ms`, `firmware_dir` (Perseus only: the user's own `fx2.hex` and `fpga.rbs`; also `RADIOCTL_PERSEUS_FIRMWARE_DIR`, else `~/.config/radioctl/firmware/perseus`). Needs `cargo build -p radioctld --features usb` (libusb-1.0); a build without it refuses the transport. `kind = "virtual"` on a USB description runs its twin on the fake bus.
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
- After a reconnect the actor unkeys the rig and calls `ProtocolEngine::link_restored`, so the Kenwood, TS-890S, Elecraft, Flex and Barrett 4100 engines send their session set-up again (manual chapter 13).

### Rotators

- A `[[rotator]]` section serves one rotator in Hamlib's `rotctld` text protocol on its own listener (default port 4533 + index).
- It runs through `radioctl-station`'s rotator engines (`rotctld` and GS-232 models from `data/station/rotators.toml`), over a reconnecting TCP link, a serial port or against the virtual rotator served inside the daemon.
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
- The CI-V keyer speed (`14 0C`) is 6 to 48 wpm, mapped to the radio's raw 0 to 250 by a measured table (unverified on a radio).

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

- C ABI sketch (own README): explicit runtime ownership, the transmit gate closed by default, rigs and rotators.
- Builds as `cdylib` and `staticlib`; header `include/radioctl.h` by cbindgen.
- Unverified sketch.

## Conventions

- **Anchors** (ADR 0090)
  - A function carries `// [@ANCHOR: id]` in source.
  - Its test carries `// Tests [@ANCHOR: id]`.
  - This README, or a story under `docs/stories/`, cites it as `[@ANCHOR: radioctl:id]`. ADR 0113 requires a full entry for every function anchor (purpose, signature, every argument, return, error and special concern, written from the code): the entries are one `###` heading per anchor in `docs/stories/radioctl_function_anchors_NN.md`, and the data structures are explained in the README next to the functions that use them.
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

The function reference moved to the story files [`docs/stories/radioctl_function_anchors_01.md`](docs/stories/) to `_43.md`: one `###` heading per anchor with its purpose, signature, arguments, returns, errors and special concerns (ADR 0113). The radiod channel anchors are in the section above.

