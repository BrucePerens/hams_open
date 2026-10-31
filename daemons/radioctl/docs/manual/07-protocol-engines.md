# 7. Protocol engines

"CI-V and CAT" are several protocols, not two. radioctl has one sans-IO engine per family in
`radioctl-protocols`; the description's `family` field picks it (`conformance::engine_for`). This chapter
says what each covers and, as important, what it does not and what nobody has checked. All engines are
**unverified**: none has talked to a real radio. Every engine is also covered by property tests that
feed it arbitrary bytes (chapter [11](11-testing.md)); reply parsing is the attack surface, so all
byte access is checked (`get`, never indexing), and no engine may panic.

| Family | Engine | Radios with a description | Protocol |
|---|---|---|---|
| `icom-civ` | `civ::CivEngine` | IC-7300, IC-705, IC-7610, IC-9700; Xiegu G90, X6100, X108G | binary frames, BCD numbers |
| `kenwood-ascii` | `kenwood::KenwoodEngine` | TS-2000, TS-590S, TS-590SG | ASCII commands ending in `;` |
| `elecraft-ascii` | `elecraft::ElecraftEngine` | KX3 (K3 and KX2 share the protocol; no description yet) | ASCII, Kenwood-like with its own replies |
| `yaesu-newcat` | `yaesu::YaesuEngine` | FT-991A, FTDX10, FTDX101D | ASCII, Kenwood-like with different widths |
| `yaesu-legacy` | `yaesu_legacy::YaesuLegacyEngine` | FT-817ND, FT-857D, FT-897D | five-byte binary blocks, unframed answers |
| `flex-smartsdr` | `flex::FlexEngine` | FLEX-6300, 6400, 6600, 6700 | SmartSDR line text over TCP (control plane only) |
| `hpsdr-network` | `hpsdr_net::HpsdrEngine` | ANAN-10, ANAN-100 | a small line protocol to a bridge over the `hpsdr` crate (control plane only) |

**Planned, not on `main` when this was written:** the Kenwood TS-890S dialect, K3/K4 descriptions, the
FT-818 (no manual found), FlexRadio streams and the 8000 series, the OpenHPSDR I/Q data plane and the
Kenwood and Icom LAN protocols.

## Icom CI-V (`civ.rs`, `civ_scope.rs`)

A frame is `FE FE <to> <from> <command> [data] FD`. A reply either echoes the command with data or is
`FB` (OK) or `FA` (NG). Frequencies are 5 bytes of BCD, least-significant pair first (14,074,000 Hz is
`00 40 07 14 00`); meters are 2 bytes of BCD. `FrameParser` splits a byte stream into frames, discards
a partial frame older than 500 ms and bounds its buffer; with `echo = true` the bus echo of our own
frame is skipped.

- **Covers:** frequency (selected VFO `03`/`05`/`25 00`, the unselected VFO `25 01`), mode (`04`, `06`),
  PTT (`1C 00`), S-meter (`15 02`), power meter (`15 11`), CW text (`17`, 1 to 30 characters), split
  (`0F`), RIT/XIT on and offset (`21`), filter width by index tables (`1A 03`), levels (`14`), model id
  (`19 00`), and unsolicited transceive frames (`00`, `01`).
- **Does not cover (deliberately not guessed):** the keyer-speed level, SWR, calibrated power and
  S-meter (the tables are per-model measurements), switching transceive on or off (`SetTransceive` is
  refused: on these radios it is a menu item reached through `1A 05` with model-specific numbers, found
  but with no field in the description format yet), data-mode and filter-number selection (`26`), the IC-9700's XIT, and the
  Main/Sub family of command `07` on the IC-7610 and IC-9700.
- **VFO labels:** `Vfo::A` is the *selected* VFO and `Vfo::B` the *unselected* one (the transmit VFO in
  split). The engine does not
  track `07 00` and `07 01`, so after the radio's selection changes the labels swap. Split on the
  Main/Sub radios differs from Hamlib's flow; the oracle test records it.
- **Scope (`civ_scope.rs`):** command `27`. Frame parsing, a reassembler that collects a whole sweep,
  settings encoders (on/off, mode, span, hold, reference level, speed, fixed edges) and a simulator. The
  engine encodes the core scope operations and returns each assembled sweep as an `Inbound::ScopeFrame`
  for the actor's scope stream ([chapter 4](04-api.md#scope-and-spectrum)); hold and speed have encoders
  but no `Op`.
- **Sources:** the basic commands come from this repository's own simulator and relay bridge; the rest
  from Hamlib 4.6.2 source for facts only. Icom's guides were not used (chapter
  [12](12-provenance-and-licensing.md)). Never verified on a radio.
- **Xiegu:** G90, X6100 and X108G use this engine with only frequency, mode, PTT and signal claimed. The
  oracle shows the X108G disagreeing with Hamlib on the final VFO A frequency; the cause is
  uninvestigated.

## Kenwood ASCII (`kenwood.rs`)

Commands are two letters, optional digits and `;` (`FA00014074000;`). Setting commands get no reply, so
the actor completes them when written; a refusal is the single token `?;`. `Tokenizer` splits the stream
on `;`.

- **Covers:** frequency `FA`/`FB`, mode `MD`, key and unkey `TX;`/`RX;`, S-meter `SM0`, `AI` (the
  transceive switch), `ID`, levels (`AG`, `RG`, `MG`, `ML`, `PC`, `KS`), RIT/XIT (`RT`, `XT`, `RC`,
  `RU`, `RD`), and the composite `IF;` answer, from which PTT, split, RIT and XIT are read.
- **`IF;`:** 38 characters on the TS-2000 and the TS-590; the engine locates the signed RIT/XIT offset as
  the sign nearest the end of the ten characters that differ between the models. The layout is from
  Kenwood's PC command references (document, page and PDF SHA-256 in each fact); the TS-2000 text was
  read from a third-party transcription of its manual.
- **Split** is `FR0;FT1;` (VFO A receives, VFO B transmits) and is read from `IF;`. `IF;` also yields
  the receive VFO (its P10 field) and, from the split flag, the transmit VFO; `FT;` reads the transmit
  VFO directly (`Op::GetTxVfo`).
- **Does not cover:** the TS-890S dialect (no `IF`, mode set with `OM`, a bandscope), CW text, power and
  SWR meters, the 590's `ML` limit of 20 on the SG (the engine takes 9), data mode (the 590's `IF` cannot be
  read in data mode), the `RU`/`RD` step (taken as hertz from a cleared offset, which the guides do not
  state outright), and the bandscope.
- **Known disagreement:** the TS-590 VFO B frequency read differs from Hamlib's (chapter [4](04-api.md)
  on issue 138) and the AF gain level differs.

## Elecraft (`elecraft.rs`)

Written from Elecraft's Programmer's Reference Rev. G5 only; no Hamlib source was read. Only the "Basic"
response formats are used and the extension modes (`K2n;`, `K3n;`) are never sent.

- **Covers:** `FA`/`FB`, `MD`, PTT read `TQ`, key and unkey (`TX`, `RX`), S-meter `SM`, power `PO`, SWR
  `SW`, bandwidth `BW` (the non-modal command, not `FW`), split `FT`/`FR` (`FT1` transmits on B; an `FT`
  answer also reports the transmit VFO), RIT/XIT `RT`, `XT`, `RO`,
  levels (`AG`, `RG`, `MG`, `ML`, `KS`, `PC`), CW text `KY`, `ID`, firmware `RV`, the composite `IF`
  decoder, and `AI` (`AI2;` to turn on specific reports, `AI0;` to turn off; `AI1;` replies are decoded
  too).
- **Safety details:** `KY` text containing `;`, `<`, `>` or control characters is refused, so a text
  cannot smuggle a second command and cannot switch TX TEST on or off. The `FR` reply is not decoded
  (the manual says its value is ignored).
- **Does not cover:** the `KY` buffer GET, `TB` text reads, menus, switch emulation, the `$` forms for
  the sub-receiver and VFO B, memories and DSP commands.
- **The first physical radio** for radioctl is a KX3 (roadmap decision, 2026-10-30). Its bench run has
  not happened.

## Yaesu newcat (`yaesu.rs`)

Written from Yaesu's CAT Operation Reference Manuals for each model (file name, PDF SHA-256 and page in
each fact).

- **Covers:** frequency (9 digits, `FA`/`FB`), mode `MD0<code>;`, PTT read `TX;`, S-meter and power
  `SM0`, `RM`, split (the FT-991A only), clarifier RIT/XIT (`RT`, `XT`, `RC`, `RU`, `RD`), levels
  (`AG0`, `RG0`, `MG`, `PC`, `KS`), model id, and `AI`.
- **Split:** the FT-991A has VFO split with `FT2;`/`FT3;`; its `FT` answer also reports the transmit VFO. The FTDX10 and FTDX101D split by band
  (MAIN/SUB transmitter), so their descriptions have no `split` capability.
- **Does not cover:** SWR and calibrated meters (the `RM` scales are model tables), VFO B reads on the
  MAIN/SUB models beyond `FB`, monitor level, the data and narrow modes (A, B, D, E, F), memory channels,
  CW text, and the FTDX101MP (its `ID` is not in the guide that was read).
- **Oracle:** the three Yaesu files agree with Hamlib 4.6.2's `rigctld` on the subset the harness
  expresses, with no known divergences.

## Yaesu legacy (`yaesu_legacy.rs`)

Every command is five bytes, four parameters and then the opcode, sent 8N2 (the description's
`stop_bits`). The radio answers a setting with nothing and a read with a fixed number of bytes and no
framing, so the engine remembers which answer it waits for and counts bytes. Written from the operating
manuals' CAT chapters (file name, PDF SHA-256, PDF and printed pages in the module comment). The notes
under the opcode chart are figures; they were read from rendered pages of the FT-817ND and FT-897D.

- **Covers:** read frequency and mode (`03`: four BCD bytes in units of 10 Hz, then the mode), set frequency
  (`01`) and mode (`07`), PTT on and off (`08`, `88`), the RX status byte (`E7`: S-meter) and TX status byte
  (`F7`: power meter, split, high SWR, PTT), split on and off (`02`, `82`), clarifier on and off (`05`, `85`)
  and offset (`F5`).
- **Frequency resolution is 10 Hz.**
- **The PTT bit level is not in the manuals** (the figure leaves it out). A clear bit 7 of the TX status
  byte means transmitting, from a comment in Hamlib 4.6.2's FT-817 backend; that single fact is labelled
  `hamlib-source` and unverified. A wrong guess here inverts the PTT read.
- **Does not cover:** a transmit-VFO report (a `TODO` in the module), a second VFO (the A/B toggle `81` is not used), lock, repeater offset, CTCSS and DCS,
  power on and off, FM-N and PKT modes, levels, and the FT-818.
- **Oracle:** Hamlib's writes agree with the virtual FT-857D and FT-897D, but its reads of frequency and mode
  fail against the virtual radio; the two steps are pinned as known. Hamlib's FT-817 backend sends an
  EEPROM read (opcode `BB`) that no manual documents, so the FT-817 is not run through the oracle. Whether
  the failed reads are a flaw in our virtual radio or a difference only a real radio shows is open.

## FlexRadio SmartSDR (`flex.rs`)

Written from FlexRadio's published SmartSDR API wiki (the commit and the pages are named in the module
comment). The wiki carries no licence or terms file; only facts (command spellings, field names, value
ranges) are used, no text. The wiki's embedded `messages.h` forbids copying and was skipped.

- **Shape:** SmartSDR has no "get" commands. The first command on a link is prefixed with the
  subscriptions (`sub slice|tx|pan all`) and `info`; the engine keeps a shadow of the status lines the
  radio pushes, and a read is a `ping` answered from the shadow. Status lines are unsolicited
  observations, so a change at the radio (a GUI) reaches the cache and events.
- **VFO A is one slice receiver** (index 0 unless set with `FlexEngine::with_slice`). The engine never
  creates or removes slices, so a radio with no slice answers reads with a timeout.
- **Covers:** frequency and mode of the slice, PTT read and `xmit 1|0`, CW (`cwx send`, up to 100
  characters), AF gain, mic gain and RF power as percentages, filter width, firmware (`version`),
  transceive (the subscriptions).
- **Does not cover:** VITA-49 UDP streams, so **no S-meter, power or SWR** (the descriptions do not claim
  `read-signal`, `read-power` or `read-swr`); slice and panadapter creation; memories; a second VFO,
  split, RIT/XIT; `keepalive`; modes the core `Mode` cannot express (`sam`, `nfm`, `dfm`, `fdv`, kept as
  text in the slice state). A discovery-packet parser exists; nothing runs discovery on the wire. The
  documented discovery port is ambiguous (4991 and 4992 appear in one paragraph) and a bench run must settle it.
- **Transmit:** `xmit 1` and `cwx send` are the only lines that can put RF on the air, from the two ops
  that `Op::transmits()` flags; a test checks that every transmitting wire line comes from such an op. CW
  text with a quote, `|`, a control character or anything outside printable ASCII is refused.
- **A virtual FlexRadio exists** (`virtual_flex.rs`, a sans-IO model of the radio's side of the TCP API
  built from the same wiki pages, and `flex_io.rs`, which serves it on a real TCP listener, one client at a
  time). The actor tests run the engine over the TCP transport against it and check on the wire that no
  keying command is sent while the gate is closed. It is our reading of the wiki, not a model of the radio.
- **Never run against a radio** (the crate README notes the radio was off). Not covered by the Hamlib oracle.

## OpenHPSDR / ANAN (`hpsdr_net.rs`, `hpsdr_io.rs`)

An OpenHPSDR radio has no command stream of the kind the actor drives: control is UDP datagrams, and
receive I/Q is a separate data plane. So the engine speaks a small line protocol (`GF`, `F <hz>`, `GP`,
`P 0|1`, `GW`, `GR`; replies `OK`, `ERR <text>`, `F <hz>`, `P <0|1>`, `W <raw>`, `R <tenths>`) to a bridge
(`hpsdr_io`) that owns an `hpsdr` crate session (Protocol 1 or 2) behind an in-memory duplex stream.

- **Covers:** frequency, PTT, forward power and SWR (the descriptions claim `read-frequency`,
  `set-frequency`, `read-ptt`, `set-ptt`, `read-power`, `read-swr`).
- **Does not cover:** a mode (an SDR's demodulation happens in the host), an attenuator (no `Op` for it
  yet), receive I/Q (the `hpsdr` crate's `IqStream` is not wired into the actor), and discovery inside
  radioctl.
- **Transmit:** `Op::SetPtt(true)` is the only command that can key, and the actor refuses it unless the
  rig's transmit gate is open. The bridge then opens the `hpsdr` crate's own, separate gate for a bounded
  lease (`HpsdrOptions::ptt_lease`), closes it on `P 0`, and unkeys by itself when the lease runs out. The
  drive level is `HpsdrOptions::drive`, zero by default.
- **Tests:** a virtual ANAN (a session against the `hpsdr` crate's loopback Protocol 1 or 2 emulator)
  and `radioctl-protocols/tests/hpsdr_actor.rs`. The `hpsdr` crate, its emulator and these files are all
  `unverified`; no HPSDR radio was used.

## What "agree" means here

The oracle (chapter [11](11-testing.md)) drives the virtual twin of a radio once through Hamlib's `rigctld`
and once through radioctl's engine and compares readings and final states. Two programs reading the same
documents can agree and both be wrong about a radio; the oracle finds typing and layout mistakes, not
facts about the hardware.
