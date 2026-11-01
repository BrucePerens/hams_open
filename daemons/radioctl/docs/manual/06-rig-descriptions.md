# 6. Rig descriptions

A rig description is a TOML file with everything radioctl knows about one model, and a record of where
each piece of knowledge came from. The same file builds the protocol engine and the virtual rig that
stands in for the model in tests, so a driver and its test double cannot drift apart. The files are in
`rigs/`; the format is implemented in `radioctl-core/src/rigfile.rs`, whose module comment is the
normative text.

A description is deliberately small. It is **data** for the parts that vary between models of one
family (addresses, identifiers, mode codes, timing, which capabilities exist). Command layouts live in
the engine of the family (chapter [7](07-protocol-engines.md)). **Planned:** the proposal also wants a
command table, meter scaling, per-firmware differences and an overridable per-model trait for odd
cases; none of that is in the format. Today a model that needs a different command is an engine
change.

## The format, field by field

Schema version 1 is the only one the reader accepts. Unknown fields and unknown names are errors
(`deny_unknown_fields`), as are duplicates and missing blocks.

| Key | Meaning |
|---|---|
| `schema` | Must be `1`. |
| `capabilities` | A non-empty list of capability names (appendix [A](A-capability-matrix.md)), without duplicates. |
| `[rig]` `id`, `name`, `family` | A stable id (`icom-ic-7300`), a display name, and the family: `icom-civ`, `kenwood-ascii`, `elecraft-ascii`, `yaesu-newcat`, `yaesu-legacy`, `flex-smartsdr`, `hpsdr-network` or `receiver`. The id and name are our own labels. |
| `[civ]` | Required for `icom-civ`, forbidden for others: `address` (the rig's CI-V address), `controller` (the software's, conventionally `0xE0`), `model_id` (what the read-ID command returns), `echo` (true if the bus echoes every frame back, as a single-wire interface does). |
| `[kenwood]`, `[elecraft]`, `[yaesu]` | `model_id`, the number `ID;` returns. Exactly the block of the family. `[kenwood]` also takes `dialect`: `classic` (the default; TS-2000, TS-590) or `ts890` (needs a `kenwood.dialect` fact). `[yaesu]` also takes `frequency_digits`, 8 or 9 (the default; 8 needs a `yaesu.frequency_digits` fact). |
| `[receiver]` | `receiver` only (AOR, Uniden, JRC, Rohde and Schwarz, Icom PCR and marine): `protocol` (`aor`, ...), `dialect` (the model's command set inside the protocol), `address` (marine and JRC models that have one), and the serial line `baud`, `data_bits`, `parity` (`N`, `E`, `O`), `stop_bits`, `flow` (`none`, `xonxoff`, `rtscts`). The daemon does not yet apply the line settings to the port. |
| `[yaesu_legacy]` | `yaesu-legacy` only: `stop_bits` (2 on the FT-817, FT-857 and FT-897). |
| `[flex]` | `flex-smartsdr` only: `model` (the name the radio reports) and `scus` (spectrum capture units, 1 or 2). |
| `[hpsdr]` | `hpsdr-network` only: `model` (`ANAN-100`), `protocol` (1 or 2) and `anan10` (true for the ANAN-10 power calibration constant). |
| `[scope]` | Optional, `icom-civ` only: `receivers` (1 or 2), `line_length`, `samples_per_frame`, `amplitude_max`, `spans_hz`. The capability `scope` is listed separately (the IC-7300 and IC-7610 do). |
| `[levels]` | Optional: `attenuator_db` (the decibels of each attenuator step, ascending, at most nine; step 0 is off), `attenuator_any_up_to_db` (instead of a list, for an attenuator that takes any whole number of decibels up to this, 31 on an ANAN; 0 otherwise) and `preamp_steps` (0 to 3 stages above off). All three keys are required when the block is present and each needs a fact (`levels.*`). Without the block the model has neither level and the engines refuse them. |
| `[timing]` | `timeout_ms` (wait for a reply), `retries` (extra attempts after a timeout; a transmit command is never repeated), `inter_command_ms` (minimum gap between writes). |
| `[defaults]` | `frequency_hz` and `mode` of a virtual rig of this model at start. They say nothing about the radio. |
| `[[modes]]` | One per supported mode: `mode` (a name from the `Mode` list in chapter [4](04-api.md): `lsb`, `usb`, `am`, `cw`, `cw-r`, `rtty`, `rtty-r`, `fm`, `wfm`, `data`, `data-r`) and `code`, either a byte (`0x01`) or a one-character string (`"1"`, for an ASCII mode digit). A mode appears once; a code appears once, except that `data` may share the code of `usb` and `data-r` that of `lsb`, listed after them (CI-V: the data mode is the sideband's code plus the DATA flag of command `26`, and a code read back without the flag is the plain mode). |
| `[[facts]]` | Provenance and verification records; see below. |

## Provenance and `verified`

Nothing in a description is taken on trust. Every datum must be covered by a `[[facts]]` entry, and
every fact must cover something: the file does not load otherwise. A fact's `key` is exact
(`civ.address`, `kenwood.model_id`, `capability.set-ptt`, `mode.usb`, `rig.id`, `timing.timeout_ms`,
`scope.line_length`) or a prefix wildcard (`capability.*`, `mode.*`, `rig.*`, `timing.*`,
`defaults.*`); an exact fact wins over a wildcard.

```toml fragment
[[facts]]
key = "capability.set-ptt"
provenance = { kind = "manual", reference = "Document title, revision, page and command", note = "Why it is trusted, or its caveat." }
verified = { state = "unverified" }
```

`provenance.kind` says what kind of source:

| Kind | Meaning, and what `reference` must name |
|---|---|
| `manual` | A vendor manual: document, revision, page, command. Add the PDF's SHA-256 if you can. |
| `capture` | A recording of a real radio: the capture. |
| `owner` | Run by the owner on a radio: who and when. |
| `repo-code` | Existing code in this repository: file and symbol. |
| `program-output` | The output of a program run: the command (for example `rigctl --dump-caps`). |
| `hamlib-source` | A fact read from Hamlib's source, used only where no vendor manual could be: Hamlib release, file and function; `note` says why a manual was not the source. |

`verified.state` says what has been checked:

| State | Needs | Means |
|---|---|---|
| `unverified` | nothing | never checked. Every fact in every shipped file is in this state today. |
| `manual-checked` | `reference` | compared with a vendor manual |
| `transcript-checked` | `reference` (the transcript) | replayed against a recorded transcript |
| `hardware-verified` | `radio`, `firmware`, `date` (`YYYY-MM-DD`), `tested` | run on a real radio; `tested` says exactly what, with which firmware |

An empty `provenance.reference` is an error, as is a `verified` state without the fields it needs. The
reader checks that a source is named, not that it is true: "from Hamlib" without a file and function
is not an acceptable reference, and review will ask for them.

## The shipped descriptions

Forty-six files. All are unverified until a volunteer confirms them (`docs/VOLUNTEER_TESTING.md`); the Icom batch of October 31, 2026 is best-effort coverage of Hamlib's models, not a claim that the radios were tried. Every family has a hand-written transcript for at least one file
(twenty-three transcripts). The FlexRadio and ANAN ones are of a different kind and say so in their
`origin`: `flex-6400-basic.toml` is the SmartSDR text protocol as radioctl's virtual FLEX-6400 speaks it
(it opens with the radio's prologue step, `V` and `H` lines, and the first command carries the engine's
subscriptions); `anan-100-basic.toml` is the line protocol between the `hpsdr-network` engine and the
`hpsdr_io` bridge, replayed against a stand-in for the bridge, because an OpenHPSDR radio has no command
stream (its control plane is UDP datagrams the `hpsdr` crate assembles). Neither says anything about a
radio; both families are also tested through the actor against virtual radios (`flex_actor.rs`,
`hpsdr_actor.rs`).

| Family | Files | Written from |
|---|---|---|
| `icom-civ` | `icom-ic-7300`, `icom-ic-705`, `icom-ic-7610`, `icom-ic-9700`, `icom-ic-7000`, `icom-ic-7100`, `icom-ic-7200`, `icom-ic-7410`, `icom-ic-7600`, `icom-ic-7700`, `icom-ic-7760`, `icom-ic-7800`, `icom-ic-7851`, `icom-ic-9100`, `icom-ic-r8600`, `icom-ic-r75`, `icom-ic-r9500`, `icom-ic-706`, `icom-ic-706mkii`, `icom-ic-706mkiig`, `icom-ic-718`, `icom-ic-746`, `icom-ic-746pro`, `icom-ic-703`, `icom-ic-756`, `icom-ic-756pro`, `icom-ic-756proii`, `icom-ic-756proiii`, `icom-ic-910`, `icom-ic-2730`, `icom-ic-78`, `icom-ic-707`, `icom-ic-729`, `icom-ic-736`, `icom-ic-737`, `icom-ic-738`, `icom-ic-775`, `icom-ic-781`, `icom-ic-970`, `icom-ic-735`, `icom-ic-905`, `icom-ic-725`, `icom-ic-726`, `icom-ic-728`, `icom-ic-751`, `icom-ic-761`, `icom-ic-765`, `icom-ic-1275`, `icom-ic-271`, `icom-ic-275`, `icom-ic-375`, `icom-ic-471`, `icom-ic-475`, `icom-ic-575`, `icom-ic-r71`, `icom-ic-r72`, `icom-ic-r7000`, `icom-ic-r7100`, `icom-ic-r8500`, `icom-ic-r9000`, `icom-ic-rx7`, `icom-id-5100`, `icom-id-1`, `icom-id-31`, `icom-id-4100`, `icom-id-51`, `icom-ic-r6`, `icom-ic-r10`, `icom-ic-r20`, `icom-ic-r30`, `icom-ic-820h`, `icom-ic-821h`, `icom-ic-92d`, `icom-ic-f8101` | Hamlib 4.6.2 source (facts only, file and function per fact) and `rigctl --dump-caps` output. Not Icom's CI-V guides. The first four have a `[scope]` block, as do the IC-7850/7851 and IC-R8600; the others have none. |
| `icom-civ` (Xiegu) | `xiegu-g90`, `xiegu-x6100`, `xiegu-x6200`, `xiegu-x108g`, `xiegu-x5105` | Hamlib facts only; deliberately small (frequency, mode, PTT, signal). |
| `kenwood-ascii` | `kenwood-ts-2000`, `kenwood-r-5000`, `kenwood-trc-80`, `kenwood-ts-930`, `kenwood-ts-590s`, `kenwood-ts-590sg`, `kenwood-ts-890s`, `kenwood-ts-50s`, `kenwood-ts-140s`, `kenwood-ts-440s`, `kenwood-ts-450s`, `kenwood-ts-480`, `kenwood-ts-570d`, `kenwood-ts-570s`, `kenwood-ts-680s`, `kenwood-ts-690s`, `kenwood-ts-711`, `kenwood-ts-790`, `kenwood-ts-811`, `kenwood-ts-850`, `kenwood-ts-870s`, `kenwood-ts-990s`, `kenwood-ts-940s`, `kenwood-ts-950s`, `kenwood-ts-950sdx` | Kenwood's published PC command references; the sixteen newer files (TS-50S to TS-950SDX) were first written from Hamlib 4.6.2 facts only (kenwood.com refuses automated downloads) and then, for the TS-480 (PC control command reference, 2003/11/28) and the TS-870S (instruction manual appendix D), compared with public copies of the manuals, which the facts name with their SHA-256 and which are not redistributed; the TS-2000 text was read from a third-party transcription of its manual, which its facts say. The TS-890S file is from its own guide (January 30, 2019), dialect `ts890`. |
| `kenwood-hh` | `kenwood-th-d7a`, `kenwood-th-f6a`, `kenwood-th-f7e`, `kenwood-th-g71`, `kenwood-tm-d700`, `kenwood-tm-v7`, `kenwood-th-d72a`, `kenwood-tm-d710`, `kenwood-tm-v71`, `kenwood-th-d74` | Hamlib 4.6.2 facts only (command strings, field columns and mode tables of `rigs/kenwood/th.c`, `thd72.c`, `thd74.c`, `tmd710.c` and the per-model files); no Kenwood manual was read and every fact is unverified. |
| `elecraft-ascii` | `elecraft-kx3`, `elecraft-k2`, `elecraft-k3`, `elecraft-k3s`, `elecraft-kx2`, `elecraft-k4` | Elecraft's Programmer's Reference, Rev. G5 (K3, K3S, KX3, KX2), cited by command heading and printed page; the K4 from its own Programmer's Reference, Rev. D11, cited by PDF page. |
| `yaesu-newcat` | `yaesu-ft-991a`, `yaesu-ftdx-9000-old`, `yaesu-ftdx10`, `yaesu-ftdx101d`, `yaesu-ft-450d`, `yaesu-ft-450`, `yaesu-ft-950`, `yaesu-ft-2000`, `yaesu-ftdx-1200`, `yaesu-ftdx-3000`, `yaesu-ftdx-5000`, `yaesu-ftdx-9000`, `yaesu-ft-891`, `yaesu-ft-710`, `yaesu-ftdx-101mp` | Yaesu's CAT Operation Reference Manuals, with page and SHA-256 per fact (the FT-450 from Hamlib facts only). The `[yaesu] frequency_digits = 8` key (default 9) selects the 8-digit frequency field of the older models. |
| `yaesu-legacy` | `yaesu-ft-817`, `yaesu-ft-818`, `yaesu-ft-857d`, `yaesu-ft-897d`, `yaesu-ft-897`, `yaesu-ft-847`, `yaesu-ft-847uni`, `yaesu-ft-650`, `yaesu-ft-736r` | Yaesu's operating manuals (CAT chapters), with page and SHA-256; the one fact the manuals leave out, the level of the PTT bit, is from Hamlib 4.6.2 and labelled `hamlib-source`. The FT-818ND file is from its own operating manual (chapter [7](07-protocol-engines.md) lists how its CAT chapter differs from the FT-817ND's), and takes the PTT level from that manual. Binary transcripts. The FT-897, FT-847, FT-847UNI, FT-650 and FT-736R files are from Hamlib 4.6.2 facts only (the FT-897 shares the FT-897D's CAT chapter); the last three are set-only. |
| `receiver` (`uniden`) | `uniden-bc245xlt`, `uniden-bc250d`, `uniden-bc780xlt`, `uniden-bc895xlt`, `uniden-bc898t` | Hamlib 4.6.2 source (`rigs/uniden/`, facts only), all `hamlib-source` and unverified; no Uniden manual was read. No transmit capability. |
| `receiver` (`jrc`) | `jrc-nrd-525`, `jrc-nrd-535d`, `jrc-nrd-545`, `jrc-jst-145`, `jrc-jst-245` | Hamlib 4.6.2 source (`rigs/jrc/`); the NRD-525 is write only; the two JST are transceivers (PTT encoded, gate closed). |
| `receiver` (`icom-pcr`) | `icom-ic-pcr100`, `icom-ic-pcr1000`, `icom-ic-pcr1500`, `icom-ic-pcr2500` | Hamlib 4.6.2 source (`rigs/pcr/`); no Icom manual was read. Frequency and mode reads return what was last set. |
| `receiver` (`icom-marine`) | `icom-ic-m700pro`, `icom-ic-m710`, `icom-ic-m802`, `icom-ic-m803` | Hamlib 4.6.2 source (`rigs/icmarine/`); `$PICOA` sentences, transceivers (PTT encoded, gate closed). |
| `receiver` (`rohde-schwarz`) | `rs-esmc`, `rs-eb200` | Hamlib 4.6.2 source (`rigs/rs/rs.c`); no Rohde and Schwarz document was read. The EK895/6, XK852 and XK2100 are open. |
| `icom-civ` (OptoScan) | `optoscan-os535`, `optoscan-os456` | Hamlib 4.6.2 source (`rigs/icom/os535.c`, `os456.c`, `optoscan.c`); CI-V at address 0x80; the OptoScan extensions are not implemented. |
| `receiver` (`aor`) | `aor-ar8000`, `aor-ar8200`, `aor-ar8600`, `aor-ar5000`, `aor-ar5000a`, `aor-ar2700`, `aor-sr2200` | Hamlib 4.6.2 source (`rigs/aor/`, facts only), all `hamlib-source` and unverified; no AOR manual was read. No transmit capability. |
| `yaesu-legacy` | `yaesu-ft-817`, `yaesu-ft-818`, `yaesu-ft-857d`, `yaesu-ft-897d` | Yaesu's operating manuals (CAT chapters), with page and SHA-256; the one fact the manuals leave out, the level of the PTT bit, is from Hamlib 4.6.2 and labelled `hamlib-source`. The FT-818ND file is from its own operating manual (chapter [7](07-protocol-engines.md) lists how its CAT chapter differs from the FT-817ND's), and takes the PTT level from that manual. Binary transcripts. |
| `flex-smartsdr` | `flex-6300`, `flex-6400`, `flex-6600`, `flex-6700` | FlexRadio's published SmartSDR API wiki (commit and pages named in `flex.rs`); control plane only. |
| `hpsdr-network` | `anan-10`, `anan-100` | The OpenHPSDR protocol work in the `hpsdr` crate (`daemons/hpsdr`, see its README for sources); control plane only. |

The Xiegu files use the Icom CI-V family because those radios speak a subset of CI-V. The X5105 and X6200
files are copies in structure of the X108G and X6100 files: Hamlib 4.6.2 gives the X5105 the X108G's settings (CI-V
address 0x70) and the X6200 the X6100's (0xA4), and no document that may be used states otherwise, so the
addresses are Hamlib's, unverified, and may well differ on the radios. Chapter [7](07-protocol-engines.md)
describes what each engine implements and chapter [12](12-provenance-and-licensing.md) why the Icom
files do not come from Icom's manuals.

## Transcripts and conformance

A transcript (`rigs/transcripts/<id>-basic.toml`) is a list of steps: a command (`op`), the bytes the
controller sends (`tx`, hex, or `tx_text` for ASCII protocols), the bytes the radio answers (`rx` or
`rx_text`), and what the engine must decode (`expect`). A step may instead be a front-panel change
(`panel`): the virtual radio's knob is turned, it sends the unsolicited bytes `rx`, and the engine must
decode them. The first step alone may be `rx` by itself: the radio's prologue, what it sends before being
asked anything (a FlexRadio's version and handle lines). `origin` is required and says honestly where
the bytes came from.

```toml transcript
schema = 1
rig = "example-er1"
origin = "hand-written for the manual; not a capture of a radio"

[[step]]
op = { kind = "get-frequency", vfo = "a" }
tx_text = "FA;"
rx_text = "FA00014074000;"
expect = { kind = "frequency", vfo = "a", hz = 14074000 }

[[step]]
op = { kind = "set-mode", mode = "usb" }
tx_text = "MD2;"
```

`radioctl_protocols::conformance::replay(description, transcript)` plays a transcript against the
engine and the virtual rig built from the same description. Per solicited step the engine must encode
the `op` to exactly `tx`, the virtual rig fed `tx` must answer exactly `rx`, and the engine fed `rx`
must decode exactly `expect`. It returns the number of steps or every mismatch. A pass means the file,
the engine and the virtual rig agree with each other. A hand-written transcript cannot show that they
agree with a radio; only a capture or an owner's bench run can, and then the `verified` field is
upgraded.

## A complete, fictional example

This file describes a radio that does not exist, "Example ER-1", in the Kenwood family. It is
**example only**: its facts cite a manual that is not real and the radio must never be added. It is
parsed by the production reader when the manual's test runs.

```toml rigfile
# EXAMPLE ONLY: a fictional radio used to show the format.
schema = 1
capabilities = ["read-frequency", "set-frequency", "read-mode", "set-mode", "set-ptt"]

[rig]
id = "example-er1"
name = "Example ER-1"
family = "kenwood-ascii"

[kenwood]
model_id = 99

[timing]
timeout_ms = 500
retries = 2
inter_command_ms = 0

[defaults]
frequency_hz = 14074000
mode = "usb"

[[modes]]
mode = "lsb"
code = "1"
[[modes]]
mode = "usb"
code = "2"
[[modes]]
mode = "cw"
code = "3"

[[facts]]
key = "rig.*"
provenance = { kind = "manual", reference = "EXAMPLE ONLY: the ER-1 CAT manual, p. 1", note = "The id and name are our own labels." }
verified = { state = "unverified" }

[[facts]]
key = "kenwood.model_id"
provenance = { kind = "manual", reference = "EXAMPLE ONLY: ER-1 CAT manual, ID command, p. 4" }
verified = { state = "unverified" }

[[facts]]
key = "timing.*"
provenance = { kind = "repo-code", reference = "daemons/radioctl/crates/radioctl-protocols/src/kenwood.rs", note = "Our choice; the manual gives no figures." }
verified = { state = "unverified" }

[[facts]]
key = "defaults.*"
provenance = { kind = "repo-code", reference = "this example", note = "Only the starting state of a virtual rig." }
verified = { state = "unverified" }

[[facts]]
key = "capability.set-ptt"
provenance = { kind = "manual", reference = "EXAMPLE ONLY: ER-1 CAT manual, TX and RX commands, p. 9", note = "Transmit-capable: the rig's transmit gate applies." }
verified = { state = "unverified" }

[[facts]]
key = "capability.*"
provenance = { kind = "manual", reference = "EXAMPLE ONLY: ER-1 CAT manual, FA, FB and MD commands, p. 6" }
verified = { state = "unverified" }

[[facts]]
key = "mode.*"
provenance = { kind = "manual", reference = "EXAMPLE ONLY: ER-1 CAT manual, MD command, p. 7: 1 LSB, 2 USB, 3 CW" }
verified = { state = "unverified" }
```

## Adding a radio, step by step

**A radio of a family that exists** (the common case). Use the Xiegu and Yaesu commits as models.

1. **Find the sources.** Prefer the vendor's published CAT or CI-V reference. Check its terms first:
   a document whose terms forbid reuse is skipped, not used and not asked about
   (chapter [12](12-provenance-and-licensing.md)). Record the title, revision, date, URL and the PDF's
   SHA-256. If no usable manual exists, Hamlib source may be read for facts only, with its release,
   file and function in each fact. Do not copy code or tables.
2. **Copy the closest shipped file** into `rigs/<make>-<model>.toml` and change it fact by fact.
   Identity (`id`, `name`, the CI-V address or model id), `timing` (from the manual's response-time
   statement), the capability list (only what the manual defines and the engine implements), the
   mode code table, and the `[scope]` block if the radio has one. Do not claim a capability the engine
   cannot encode; the engine will return `Unsupported` for it and the description will have lied.
3. **Mark every fact `unverified`** unless you ran it. Never write `manual-checked`,
   `transcript-checked` or `hardware-verified` for something you did not check.
4. **Write the transcript** `rigs/transcripts/<id>-basic.toml` by hand from the manual's examples.
   State in `origin` that it is hand-written.
5. **Run the checks.** `cargo run -p radioctld --example rig_file -- <id>` loads the file, prints
   the verified states, and replays the transcript (it needs `rigs/transcripts/<id>-basic.toml`):

   ```text
   Elecraft KX3: family ElecraftAscii
   facts by verified state: {"unverified": 20}
   transcript origin: hand-written from the Elecraft Programmer's Reference Rev. G5; not a capture of a radio
   replayed 44 steps: engine, virtual rig and file agree
   ```

   The workspace tests (`tests/rig_files.rs`) load every file in `rigs/` and replay every transcript
   in `rigs/transcripts/`, so a new file is checked without further wiring.
6. **Compare with Hamlib** where the oracle can: add the file to the relevant `*_divergences` list in
   `crates/radioctl-protocols/tests/rigctld_oracle.rs` (chapter [11](11-testing.md)) and record each
   disagreement with the reason.
7. **Document and merge** following the repository rules: a pull request, the linters, the test
   results in the PR text, and the manual's chapter 6 table updated.

**A new family** (a radio with a different protocol). The Yaesu newcat, Yaesu legacy and Flex commits
touched these places, in this order; the compiler finds most of them if you add the variant first.

1. `radioctl-core/src/description.rs`: a `Family` variant and a parameters struct (such as `YaesuParams`)
   and the field in `RigDescription`.
2. `radioctl-core/src/rigfile.rs`: the family name, the optional block, the match that requires exactly
   the family's block, and `expected_keys`.
3. `radioctl-protocols/src/<family>.rs`: the sans-IO engine (`ProtocolEngine`), with checked byte
   access only, no panics.
4. `radioctl-protocols/src/virtual_rig.rs`: the simulated radio of the family, including the fixed
   answers Hamlib's backend needs at start-up, so the oracle can run.
5. `radioctl-protocols/src/conformance.rs`: `engine_for`.
6. `radioctld/src/names.rs`: `level_range` for the family, so levels scale correctly.
7. Descriptions, transcripts, oracle tests and a README paragraph, as above.

## Verifying on real hardware and recording it

This is the step that makes a fact worth more than a hand-written transcript. It is for a radio's
owner.

1. Agree the plan with the owner first. Read before you write, and never key a transmitter in a test:
   use a dummy load only with the owner's explicit decision, and keep the transmit gate closed
   everywhere else.
2. Capture the exchange (a serial tap, or the transcript tail of a `RigError`), and keep the capture
   free of serial numbers, keys and addresses.
3. For each fact you exercised, change `verified` to `hardware-verified` with the radio, its firmware
   version, the date and what exactly you tested:

   ```toml fragment
   # Illustrative values, not a real test run.
   verified = { state = "hardware-verified", radio = "Elecraft KX3 (owner's)", firmware = "02.37", date = "2026-11-02", tested = "FA and FB reads and sets on 20 m, MD cycle, TQ read; no transmit" }
   ```
4. Add the capture as a transcript and its origin as `capture`.
5. Say in the pull request what was run. "Fixtures only" is an acceptable statement; an untrue date is
   not.
