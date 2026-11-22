# 7. Protocol engines

"CI-V and CAT" are several protocols, not two. radioctl has one sans-IO engine per family in
`radioctl-protocols`; the description's `family` field picks it (`conformance::engine_for`). This chapter
says what each covers and, as important, what it does not and what nobody has checked. All engines are
**unverified** except the Elecraft one, which has talked to one real radio (a KX3, bench run 2026-11-05; the
facts that run exercised are `hardware-verified` in `rigs/elecraft-kx3.toml`). No other engine has talked to a real radio. Every engine is also covered by property tests that
feed it arbitrary bytes (chapter [11](11-testing.md)); reply parsing is the attack surface, so all
byte access is checked (`get`, never indexing), and no engine may panic.

The table lists the first radios of each family; the complete list of every described radio, with the family
and the engine of each, is the generated [`RIG_SUPPORT.md`](../RIG_SUPPORT.md) (chapter [6](06-rig-descriptions.md) has
the file families). Families added since the table was written (the small serial-vendor engines, `kenwood-hh`,
`usb-device`, `radiod-channel`) are described there and in the sections below.

| Family | Engine | Radios with a description | Protocol |
|---|---|---|---|
| `icom-civ` | `civ::CivEngine` | IC-7300, IC-705, IC-7610, IC-9700; Xiegu G90, X6100, X6200, X108G, X5105 | binary frames, BCD numbers |
| `kenwood-ascii` | `kenwood::KenwoodEngine` | TS-2000, TS-590S, TS-590SG | ASCII commands ending in `;` |
| `kenwood-ascii`, `dialect = "ts890"` | `kenwood_ts890::Ts890Engine` | TS-890S | the same framing, its own command set |
| `elecraft-ascii` | `elecraft::ElecraftEngine` | KX3, KX2, K2, K3, K3S, K4 | ASCII, Kenwood-like with its own replies |
| `yaesu-newcat` | `yaesu::YaesuEngine` | FT-991A, FTDX10, FTDX101D | ASCII, Kenwood-like with different widths |
| `receiver` (`aor`) | `aor::AorEngine` (a `receiver::LineEngine`) | AR8000, AR8200, AR8600, AR5000, AR5000A, AR2700, SR2200 | ASCII lines ended by CR (`RF`, `RX`, `MD`, `LM`); a setting is acknowledged by an empty line, `?` refuses |
| `receiver` (`uniden`) | `uniden::UnidenEngine` | BC245XLT, BC250D, BC780XLT, BC895XLT, BC898T | ASCII lines ended by CR (`RF`, `RM`, `SG`), `OK` or `NG`/`ERR` |
| `receiver` (`jrc`) | `jrc::JrcEngine` | NRD-525, NRD-535D, NRD-545 DSP, JST-145, JST-245 | ASCII commands ended by CR (`F`, `D`, `I`, `M`, `X`); settings are not answered |
| `receiver` (`icom-pcr`) | `icom_pcr::PcrEngine` | IC-PCR100, PCR1000, PCR1500, PCR2500 | ASCII commands ended by LF (`K0` tune, `I1?` signal), `G000`/`G001` answers |
| `receiver` (`icom-marine`) | `icom_marine::MarineEngine` | IC-M700PRO, M710, M802, M803 | NMEA-style `$PICOA` sentences with XOR checksum, the radio echoes a setting |
| `receiver` (`rohde-schwarz`) | `rs::RsEngine` | ESMC, EB200 | ASCII `FREQ`, `DEM`, `SENS` commands framed by CR; settings are not answered |
| `yaesu-legacy` | `yaesu_legacy::YaesuLegacyEngine` | FT-817ND, FT-818ND, FT-857D, FT-897D | five-byte binary blocks, unframed answers |
| `flex-smartsdr` | `flex::FlexEngine` | FLEX-6300, 6400, 6600, 6700 | SmartSDR line text over TCP (control plane only) |
| `hpsdr-network` | `hpsdr_net::HpsdrEngine` | ANAN-10, ANAN-100 | a small line protocol to a bridge over the `hpsdr` crate (control plane only) |

**Planned, not on `main`:** the
FlexRadio streams and the 8000 series, the OpenHPSDR I/Q data plane, the Icom LAN protocol
and the Kenwood LAN protocol of radios other than the TS-890S (the TS-890S session exists, chapter [5](05-transports.md)).

## Receive attenuator and preamplifier (`attenuation.rs`)

`Level::Attenuator` is decibels of attenuation (0 is off) and `Level::Preamp` a step number (0 is off;
1 and 2 are the rig's first and second stage; a Yaesu "IPO" is step 0). A stage's gain varies by band and
model and no source we may use states it, so the preamplifier is not in decibels, and radioctld serves it
as a step (Hamlib's own `PREAMP` is in decibels: a Hamlib client sees the step numbers in the lists of
`\dump_state`). The values a model has are its description's `[levels]` block; `attenuation::step_of`
turns a value into the wire step and refuses any value the block does not list, so no engine carries a
table of its own. The wire forms are:

| Family | Attenuator | Preamplifier | Models with a `[levels]` block |
|---|---|---|---|
| `icom-civ` | `11` with one BCD byte of decibels | `16 02` with 0, 1 (P.AMP1) or 2 (P.AMP2) | IC-7300, IC-705, IC-7610, IC-9700 |
| `kenwood-ascii` | `RAnn;` (00 off, 01 on), answer `RAnn00;` | `PAn;`, answer `PAn0;` | TS-590S, TS-590SG |
| `elecraft-ascii` | `RAnn;` (00 off, 01 on, 10 dB) | `PAn;` | KX3 |
| `yaesu-newcat` | `RA0n;` (`n` the step: 0 off, 1 on or 6 dB, 2 12 dB, 3 18 dB) | `PA0n;` (0 IPO, 1 AMP 1, 2 AMP 2) | FT-991A, FTDX10, FTDX101D (main band) |
| `hpsdr-network` | line `A <db>`, 0 to 31 | none | ANAN-10, ANAN-100 |

The TS-2000 has no block (its command text is known only from a third-party transcription), the Xiegu
radios and the FT-817, FT-857D and FT-897D have none (the Xiegu descriptions are deliberately minimal;
the legacy five-byte family has no attenuator or preamplifier command), and the Flex engine refuses both.
The attenuation of the on-or-off attenuators (12 dB on the Kenwood TS-590 and the FT-991A) is Hamlib's
figure, read for facts only and marked as such in the description.

## Icom CI-V (`civ.rs`, `civ_scope.rs`)

A frame is `FE FE <to> <from> <command> [data] FD`. A reply either echoes the command with data or is
`FB` (OK) or `FA` (NG). Frequencies are 5 bytes of BCD, least-significant pair first (14,074,000 Hz is
`00 40 07 14 00`); meters are 2 bytes of BCD. `FrameParser` splits a byte stream into frames, discards
a partial frame older than 500 ms and bounds its buffer; with `echo = true` the bus echo of our own
frame is skipped.

- **Covers:** frequency (selected VFO `03`/`05`/`25 00`, the unselected VFO `25 01`), mode (`04`, `06`;
  on a description that lists `data` and `data-r`, `26 00` read and `26 00 <mode> <data> <filter>` set,
  which carry the DATA flag: `Mode::Data` is USB-D, `Mode::DataR` LSB-D, and the filter byte is the filter
  number last reported, 1 before any read), PTT (`1C 00`), S-meter (`15 02`), power meter (`15 11`), the
  raw meters SWR, ALC, COMP, Vd and Id (`15 12` to `15 16`, `Op::GetMeter`, a 0 to 255 deflection;
  read-only, gate closed or open), CW text (`17`, 1 to 30 characters) and its stop (`17 FF`), the unselected
  VFO's frequency read and set (`25 01`), split (`0F`), RIT/XIT on and offset (`21`), filter width by index
  tables (`1A 03`), levels (`14`: AF gain, RF gain, RF power, mic gain, monitor level and the keyer speed,
  6 to 48 wpm), the attenuator (`11`) and preamplifier (`16 02`) of a description with a `[levels]` block,
  model id (`19 00`), the Main/Sub commands of command `07` (select the band `D0`/`D1` on a description with
  `main_sub = "select"`, `"select-read"` or `"full"`; read the selected band `D2` on the last two and dual watch `C0` to `C2` on `"full"`: IC-7610,
  IC-7600, IC-7800, IC-7760; `"select-read"`: IC-9700, IC-7851), the core scope operations (command `27`, below), the IC-F8101's own frequency and mode commands
  (`1A 34` to `1A 36`), the repeater and tone ops of the `repeater` capability (shift `0F 10`/`11`/`12`,
  offset `0C` read and `0D` set as three BCD bytes of 100 Hz units, CTCSS tone `1B 00` in tenths of a hertz,
  DCS code `1B 02`, and the tone mode as the three flags `16 42` tone, `16 43` tone squelch and `16 4B`
  DCS: a mode set sends the three frames at once and the engine passes on the first acknowledgement and
  absorbs the other two, a mode read asks the three flags and reports the mode when the third answers;
  facts from Hamlib 4.6.2, unverified on a radio, and Hamlib notes the shift read does not work on an
  IC-706MkIIG), and unsolicited transceive frames (`00`, `01`).
<!--ops rig=icom-ic-7610: ?GetFilterWidth ?SetFilterWidth GetCtcssTone GetDcsCode GetDualWatch GetFrequency GetFrequency.B GetLevel.AfGain GetLevel.Attenuator GetLevel.KeyerSpeed GetLevel.MicGain GetLevel.MonitorLevel GetLevel.Preamp GetLevel.RfGain GetLevel.RfPower GetMeter GetMode GetModelId GetPower GetPtt GetReceiver GetRepeaterOffset GetRepeaterShift GetRit GetRitOffset GetScopeAxis GetScopeOutput GetScopeReference GetScopeSpan GetSignal GetSplit GetToneMode GetXit SelectReceiver SendCw SetCtcssTone SetDcsCode SetDualWatch SetFrequency SetFrequency.B SetLevel.AfGain SetLevel.Attenuator SetLevel.KeyerSpeed SetLevel.MicGain SetLevel.MonitorLevel SetLevel.Preamp SetLevel.RfGain SetLevel.RfPower SetMode SetPtt SetRepeaterOffset SetRepeaterShift SetRit SetRitOffset SetScopeAxis SetScopeEdges SetScopeOutput SetScopeReference SetScopeSpan SetSplit SetToneMode SetXit StopCw-->
- **Does not cover (deliberately not guessed):** the calibrated SWR ratio
  (`Op::GetSwr`), power and S-meter (Hamlib's tables are per-model measurements; radioctl reports the raw
  deflection instead), switching transceive on or off (`SetTransceive` is
  refused: on these radios it is a menu item reached through `1A 05` with model-specific numbers, found
  but with no field in the description format yet), filter-number selection, AM-D and FM-D (no mode
  name for them), the older `1A 06` data-mode form, the IC-9700's XIT, the
  transmit-band selection of the Main/Sub radios (`SetTxReceiver`, `GetTxReceiver`), and the Main/Sub
  commands on a description with no `main_sub` key (every model but the IC-7610, IC-7600, IC-7800, IC-7760, IC-7851, IC-9700 and IC-9100). Whether `06` clears DATA on a real radio,
  and whether the filter byte of `26 00` may be left out, are IC-7300 bench questions.
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
  from Hamlib 4.6.2 source for facts only. Icom's manuals were read for facts of the later models, under Bruce
  Perens' decision of 2026-10-31 (chapter [12](12-provenance-and-licensing.md)). Never verified on a radio.
- **Xiegu:** G90, X6100, X6200, X108G and X5105 use this engine with only frequency, mode, PTT and signal claimed. The
  oracle shows the X108G and the X5105 (which Hamlib gives the X108G's settings) disagreeing with Hamlib on
  the final VFO A frequency: Hamlib selects VFO B first and writes there, radioctl never sends `07`. The G90, X6100
  and X6200 agree.

## Kenwood ASCII (`kenwood.rs`)

Commands are two letters, optional digits and `;` (`FA00014074000;`). Setting commands get no reply, so
the actor completes them when written; a refusal is the single token `?;`. `Tokenizer` splits the stream
on `;`.

- **Covers:** frequency `FA`/`FB`, mode `MD`, key and unkey `TX;`/`RX;`, S-meter `SM0`, `AI` (the
  transceive switch), `ID`, levels (`AG`, `RG`, `MG`, `ML`, `PC`, `KS`), RIT/XIT (`RT`, `XT`, `RC`,
  `RU`, `RD`), the attenuator and preamplifier (`RA`, `PA`) and the antenna port (`AN`) of the models whose
  description has them, CW text and its stop (`KY`, only on a model whose description has a `[kenwood.ky]`
  block), and the composite `IF;` answer, from which PTT, split, RIT and XIT are read. The older TS-440S
  and the TS-990S have their own small command tables inside this engine and refuse what those books lack.
  The tone ops of the `repeater` capability on the TS-590S only: the tone mode as the flags `TO` (tone) and
  `CT` (CTCSS tone squelch) sent together and read together (`Off`, `Tone`, `Tsql`), and the tone `TN` (with
  `CN` for the squelch, up to number 41) by the guide's chart. The TS-590S has no DCS and no repeater shift or
  offset command, so those ops are refused. Unverified on a radio.
<!--ops rig=kenwood-ts-590s: GetCtcssTone GetFrequency GetFrequency.B GetLevel.AfGain GetLevel.Antenna GetLevel.Attenuator GetLevel.KeyerSpeed GetLevel.MicGain GetLevel.MonitorLevel GetLevel.Preamp GetLevel.RfGain GetLevel.RfPower GetMode GetModelId GetPtt GetRit GetRitOffset GetSignal GetSplit GetToneMode GetTxVfo GetXit SendCw SetCtcssTone SetFrequency SetFrequency.B SetLevel.AfGain SetLevel.Antenna SetLevel.Attenuator SetLevel.KeyerSpeed SetLevel.MicGain SetLevel.MonitorLevel SetLevel.Preamp SetLevel.RfGain SetLevel.RfPower SetMode SetPtt SetRit SetRitOffset SetSplit SetToneMode SetTransceive SetXit StopCw-->
- **`IF;`:** 38 characters on the TS-2000 and the TS-590; the engine locates the signed RIT/XIT offset as
  the sign nearest the end of the ten characters that differ between the models. The layout is from
  Kenwood's PC command references (document, page and PDF SHA-256 in each fact); the TS-2000 text was
  read from a third-party transcription of its manual.
- **Split** is `FR0;FT1;` (VFO A receives, VFO B transmits) and is read from `IF;`. `IF;` also yields
  the receive VFO (its P10 field) and, from the split flag, the transmit VFO; `FT;` reads the transmit
  VFO directly (`Op::GetTxVfo`).
- **Does not cover:** the TS-890S dialect (its own engine, below), CW text on a model whose description has
  no `[kenwood.ky]` block, power and SWR meters, the 590's `ML` limit of 20 on the SG (the engine takes 9), data mode (the 590's `IF` cannot be
  read in data mode), the `RU`/`RD` step (taken as hertz from a cleared offset, which the guides do not
  state outright), and the bandscope.
- **Known disagreement:** the TS-590 VFO B frequency read differs from Hamlib's (chapter [4](04-api.md)
  on issue 138) and the AF gain level differs.

## Kenwood TS-890S (`kenwood_ts890.rs`)

The TS-890S shares the framing of the classic Kenwood family (`;`-ended ASCII, `?;` for a refusal) and
nothing else of the command set, so it has its own engine, chosen by `[kenwood] dialect = "ts890"` in the
description (the classic engine refuses such a description, and `conformance::engine_for` picks this one).
Source: Kenwood's "TS-890S PC CONTROL COMMAND Reference Guide", January/30/2019 (page and SHA-256 in each
fact of `rigs/kenwood-ts-890s.toml`).

- **Differs from the classic family:** no `IF` (split is `TB`, the VFOs `FR` and `FT`, RIT and XIT `RT` and
  `XT`, the RIT/XIT offset is read with `RF` and set with `RC` then `RU` or `RD`); the mode is `OM0;` and
  `OM0<m>;` where the code is 1 LSB, 2 USB, 3 CW, 4 FM, 5 AM, 6 FSK, 7 CW-R, 9 FSK-R, C LSB-D, D USB-D (no
  `MD`); `AG` has no receiver digit; `KS` runs from 4; there is no monitor level; `SM;` answers four digits; `RA`
  and `PA` take one digit (attenuator 0 off, 6, 12 or 18 dB; preamplifier 0 off, PRE 1, PRE 2). The transmit
  state has no read command, so the description has no `read-ptt` (the actor learns the state from what it
  sent). `ID;` answers `ID024;`.
- **Covers:** frequency `FA`/`FB`, mode `OM0`, key and unkey (`TX0;`, `RX;`), S-meter `SM`, `AI` (the
  transceive switch), split `TB`, the transmit VFO `FT`, RIT/XIT, levels (`AG`, `RG`, `MG`, `KS`, `PC`),
  the attenuator and preamplifier (`RA`, `PA`), the antenna port (`AN`), CW text and its stop (`KY`), `ID`,
  and the bandscope operations listed below (output, axis, span, reference level, edges, settings, actions).
  The tone ops of the `repeater` capability: the tone function `TO` (0 off, 1 tone, 2 CTCSS, which is `Off`, `Tone`
  and `Tsql`; cross tone, 3, is not mapped) and the tone `TN` with `CN` for the squelch (number 49 at most) by the
  guide's 51-entry chart. The guide has no DCS, repeater shift or offset command, so those ops are refused.
  Unverified on a radio.
<!--ops rig=kenwood-ts-890s: GetCtcssTone GetFrequency GetFrequency.B GetLevel.AfGain GetLevel.Antenna GetLevel.Attenuator GetLevel.KeyerSpeed GetLevel.MicGain GetLevel.Preamp GetLevel.RfGain GetLevel.RfPower GetMode GetModelId GetRit GetRitOffset GetScopeAxis GetScopeOutput GetScopeReference GetScopeSetting GetScopeSpan GetSignal GetSplit GetToneMode GetTxVfo GetXit ScopeAction SendCw SetCtcssTone SetFrequency SetFrequency.B SetLevel.AfGain SetLevel.Antenna SetLevel.Attenuator SetLevel.KeyerSpeed SetLevel.MicGain SetLevel.Preamp SetLevel.RfGain SetLevel.RfPower SetMode SetPtt SetRit SetRitOffset SetScopeAxis SetScopeEdges SetScopeOutput SetScopeReference SetScopeSetting SetScopeSpan SetSplit SetToneMode SetTransceive SetXit StopCw-->
- **LAN:** the guide documents a TCP connection (UTF-8; the radio drops it after 10 s of silence): `##CN;`
  then `##ID` with an account type, the two lengths, the account and the password. The frames are built by
  `lan_connect_request` and `lan_login` and the answers read by `lan_reply`. `Ts890LanFraming` (`kenwood_ts890_lan`) runs them over the network link and `radioctld` serves it as
  `framing = "ts890-lan"` (chapter [9](09-radioctld.md)); tested against a fake radio only. A session needs the owner's administrator or user account from the radio's LAN menu, which belongs in the
  station's secret store and never in a description. The commands after login are those of the serial link.
- **Bandscope (unverified, virtual rig only):** the core scope operations map to `BS3` (center, fixed, auto
  scroll), `BS4` (span 5 to 500 kHz), `BSC` (reference level, half-decibel steps from -20 to +10 dB) and
  `DD0` (waveform output; the engine sets LAN medium cycle) and `DD1` (filter-scope output, which `##DD3` needs; the engine sets LAN). The LAN-only `##DD2` frame (640 hexadecimal
  samples, 00 strongest to 8C weakest, read from the August 2018 guide, `ts890_pc_command_e.pdf`) becomes a
  `ScopeFrame` for the main receiver, placed from the last `FA`/`FB`, `FR`, `BS3` and `BS4` the engine saw
  (turning the output on reads them first) in center mode, and in fixed mode from the edges of the last
  `BSM` read; in auto-scroll mode from the `BSM0` read too, flagged `out_of_range` once the receive frequency
  leaves those edges (the engine reads `BSM` again after each frequency it sets while scrolling; the guide does not say
  the rig pushes new edges, so a dial turn on the rig leaves them stale until the next read). Sweeps before the
  placement is known are counted and dropped. The rig sends frames only while auto information (`transceive`) is
  on. `BS0`, `BS1`, `BS5`, `BS6` to `BS9`, `BSA`, `BSB`, `BSG` to `BSK`, `BSN` and `BSO` are scope settings
  (`Op::SetScopeSetting`, the guide's digit is the code), `BSL` (overflow) is one that can only be read, and `BSD`
  (waterfall clear) and `BSE` (marker shift or centre, refused in center mode) are `Op::ScopeAction`; `BSM` stores the
  edges of fixed range 1 to 3 (`Op::SetScopeEdges`). `##DD3` (filter scope, 213 samples) becomes a
  `FilterScopeFrame` on its own stream (`RigHandle::filter_scope_frames`). With the expanded analysis range
  (`BSO`) on, a center-mode sweep is placed over the width the guide gives (100 kHz span: 300 kHz, 200 kHz: 400 kHz);
  any other expanded case, expanded scroll mode or an unread `BSO` is dropped. The virtual rig answers every `BS`
  command above, `BSM`, `DD0` and `DD1` and sends synthetic `##DD2` and `##DD3` frames; its auto-scroll edges are our model
  (a page of the span's width), since the guide gives none.
- **Does not cover:** the COM/USB `DD4` form (the guide bars AI on that connector, which the engine needs); the COM/USB `DD2`/`DD3` splits are handled when the description says `kenwood.scope_link = "serial"` (the engine then sets `DD0` 4 and `DD1` 2; the default `lan` sets `DD0` 2 and `DD1` 1; virtual rig only, unverified on a radio); the audio scope and oscilloscope waveforms;
  the monitor level, memories, the menu (`EX`) and the `PSQ/PKS` pin note.
- **Oracle:** Hamlib 4.6.2's TS-890S backend reads the frequency and mode with `IF;` and sets the frequency
  with `SF`, neither of which is in the guide, so the virtual rig refuses them and the frequency and mode steps
  diverge (`rigctld_oracle.rs` records which). A bench run would show whether a real TS-890S answers `IF;`;
  until then radioctl follows the guide.

  The TS-990S has its own LAN session, `Ts990LanFraming` (`kenwood_ts990_lan`): the same `##CN;`, then `##ID` with two one-digit lengths (no account type), UTF-16 text on the wire (PDF p. 1 gives UTF-16 but no byte order; little endian is used, unverified), an `ID;` keep-alive. `radioctld` serves it as `framing = "ts990-lan"` with `lan_account` and `lan_password_secret` (chapter [9](09-radioctld.md)); fake radio only, unverified.

## Elecraft (`elecraft.rs`)

Written from Elecraft's Programmer's Reference Rev. G5 only; no Hamlib source was read. Only the "Basic"
response formats are used and the extension modes (`K2n;`, `K3n;`) are never sent.

- **Covers:** `FA`/`FB`, `MD`, PTT read `TQ`, key and unkey (`TX`, `RX`), S-meter `SM`, power `PO`, SWR
  `SW`, bandwidth `BW` (the non-modal command, not `FW`), split `FT`/`FR` (`FT1` transmits on B; an `FT`
  answer also reports the transmit VFO), RIT/XIT `RT`, `XT`, `RO`,
  levels (`AG`, `RG`, `MG`, `ML`, `KS`, `PC`), the attenuator and preamplifier (`RA`, `PA`) and the antenna
  port (`AN`) where the description lists them, CW text `KY` and its stop, `ID`, firmware `RVM`, whether a
  power amplifier is installed (`OM`), the composite `IF` decoder, and `AI` (`AI2;` to turn on specific
  reports, `AI0;` to turn off; `AI1;` replies are decoded too).
<!--ops rig=elecraft-kx3: GetFilterWidth GetFirmware GetFrequency GetFrequency.B GetLevel.AfGain GetLevel.Attenuator GetLevel.KeyerSpeed GetLevel.MicGain GetLevel.MonitorLevel GetLevel.Preamp GetLevel.RfGain GetLevel.RfPower GetMode GetModelId GetPower GetPowerAmplifier GetPtt GetRit GetRitOffset GetSignal GetSplit GetSwr GetTxVfo GetXit SendCw SetFilterWidth SetFrequency SetFrequency.B SetLevel.AfGain SetLevel.Attenuator SetLevel.KeyerSpeed SetLevel.MicGain SetLevel.MonitorLevel SetLevel.Preamp SetLevel.RfGain SetLevel.RfPower SetMode SetPtt SetRit SetRitOffset SetSplit SetTransceive SetXit StopCw-->
- **Safety details:** `KY` text containing `;`, `<`, `>` or control characters is refused, so a text
  cannot smuggle a second command and cannot switch TX TEST on or off. The `FR` reply is not decoded
  (the manual says its value is ignored).
- **Does not cover:** the `KY` buffer GET, `TB` text reads, menus, switch emulation, the `$` forms for
  the sub-receiver and VFO B, memories and DSP commands.
- **The first physical radio** for radioctl was a KX3 (roadmap decision, 2026-10-30). Its bench run took
  place on 2026-11-05; chapter [6](06-rig-descriptions.md) and `rigs/reports/verified.toml` say what was
  verified.

## Yaesu newcat (`yaesu.rs`)

Written from Yaesu's CAT Operation Reference Manuals for each model (file name, PDF SHA-256 and page in
each fact).

- **Covers:** frequency (9 digits, `FA`/`FB`), mode `MD0<code>;`, PTT read `TX;` and key and unkey
  (`TX1;`, `TX0;`), S-meter and power `SM0`, `RM`, split (`FT2;`/`FT3;` or `ST`, below), clarifier RIT/XIT
  (`RT`, `XT`, `RC`, `RU`, `RD`; the FT-710 family's `CF`), levels (`AG0`, `RG0`, `MG`, `PC`, `KS`), the
  attenuator and preamplifier (`RA0`, `PA0`) and the antenna port (`AN`) where the description lists them,
  model id, `AI`, and on the models whose `FT` picks the transmitting band (FT-2000, FTDX5000, FTDX9000) the
  transmit receiver, Main or Sub.
  The repeater and tone ops of the `repeater` capability (listed for the FT-991A and the FT-891): shift
  `OS0` (0 simplex, 1 plus, 2 minus), tone mode `CT0` (0 off, 1 CTCSS encode and decode, 2 CTCSS encode,
  3 DCS), the CTCSS tone `CN00` and the DCS code `CN01` by their number in the book's charts (tenths of a
  hertz and decimal-octal codes in the API). The books have no offset command: it is a menu item, so the
  offset ops are refused. Unverified on a radio.
<!--ops rig=yaesu-ft-991a: GetCtcssTone GetDcsCode GetFrequency GetFrequency.B GetLevel.AfGain GetLevel.Attenuator GetLevel.KeyerSpeed GetLevel.MicGain GetLevel.Preamp GetLevel.RfGain GetLevel.RfPower GetMode GetModelId GetPower GetPtt GetRepeaterShift GetRit GetRitOffset GetSignal GetSplit GetToneMode GetTxVfo GetXit SetCtcssTone SetDcsCode SetFrequency SetFrequency.B SetLevel.AfGain SetLevel.Attenuator SetLevel.KeyerSpeed SetLevel.MicGain SetLevel.Preamp SetLevel.RfGain SetLevel.RfPower SetMode SetPtt SetRepeaterShift SetRit SetRitOffset SetSplit SetToneMode SetTransceive SetXit-->
<!--ops rig=yaesu-ftdx-5000: GetCtcssTone GetDcsCode GetFrequency GetFrequency.B GetLevel.AfGain GetLevel.KeyerSpeed GetLevel.MicGain GetLevel.RfGain GetLevel.RfPower GetMode GetModelId GetPower GetPtt GetRepeaterShift GetRit GetRitOffset GetSignal GetSplit GetToneMode GetTxReceiver GetTxVfo GetXit SetCtcssTone SetDcsCode SetFrequency SetFrequency.B SetLevel.AfGain SetLevel.KeyerSpeed SetLevel.MicGain SetLevel.RfGain SetLevel.RfPower SetMode SetPtt SetRepeaterShift SetRit SetRitOffset SetSplit SetToneMode SetTransceive SetTxReceiver SetXit-->
- **Split:** the FT-991A has VFO split with `FT2;`/`FT3;`; its `FT` answer also reports the transmit VFO. The FTDX10, FTDX101D and
  FT-710 set split with `ST` (their `[yaesu] split = "st"`), and the FTDX5000 and its relatives pick the
  transmitting band instead (`tx_band`).
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
<!--ops rig=yaesu-ft-817: GetFrequency GetMode GetPower GetPtt GetSignal GetSplit SetFrequency SetMode SetPtt SetRit SetRitOffset SetSplit-->
- **Frequency resolution is 10 Hz.**
- **The PTT bit level is not in the manuals** (the figure leaves it out). A clear bit 7 of the TX status
  byte means transmitting, from a comment in Hamlib 4.6.2's FT-817 backend; that single fact is labelled
  `hamlib-source` and unverified. A wrong guess here inverts the PTT read.
- **Does not cover:** a transmit-VFO report (a `TODO` in the module), a second VFO (the A/B toggle `81` is not used), lock, repeater offset, CTCSS and DCS,
  power on and off, FM-N and PKT modes, and levels.
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
  transceive (the subscriptions), and the stop of CW text (`cwx clear`).
<!--ops rig=flex-6300: ?SetFilterWidth GetFilterWidth GetFirmware GetFrequency GetLevel.AfGain GetLevel.MicGain GetLevel.RfPower GetMode GetPtt SendCw SetFrequency SetLevel.AfGain SetLevel.MicGain SetLevel.RfPower SetMode SetPtt SetTransceive StopCw-->
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
  `set-frequency`, `read-ptt`, `set-ptt`, `read-power`, `read-swr`), and the attenuator below.
<!--ops rig=anan-100: GetFrequency GetLevel.Attenuator GetPower GetPtt GetSwr SetFrequency SetLevel.Attenuator SetPtt-->
- **Attenuator:** `Level::Attenuator` in whole decibels, 0 to 31 (`GA` and `A <db>` on the bridge's line
  protocol; the same call as `HpsdrControl::set_attenuator`, which remains for code that holds the handle).
- **Does not cover:** a mode (an SDR's demodulation happens in the host), a preamplifier, receive I/Q (the `hpsdr` crate's `IqStream` is not wired into the actor), and discovery inside
  radioctl.
- **Transmit:** `Op::SetPtt(true)` is the only command that can key, and the actor refuses it unless the
  rig's transmit gate is open. The bridge then opens the `hpsdr` crate's own, separate gate for a bounded
  lease (`HpsdrOptions::ptt_lease`), closes it on `P 0`, and unkeys by itself when the lease runs out. The
  drive level is `HpsdrOptions::drive`, zero by default.
- **Tests:** a virtual ANAN (a session against the `hpsdr` crate's loopback Protocol 1 or 2 emulator)
  and `radioctl-protocols/tests/hpsdr_actor.rs`. The `hpsdr` crate, its emulator and these files are all
  `unverified`; no HPSDR radio was used.

## Other engines and shared parts

Each of these is a sans-IO engine with a virtual twin and tests against the twin; none has been run on a
radio. The `Sources` paragraph at the top of each file names the manual or header the facts came from.

- **`aor_classic.rs`**: AOR AR3030 and AR3000A, whose ASCII sets differ from the `RF` / `MD` / `LM` family of
  `aor.rs`.
- **`rs_hf.rs`**: Rohde and Schwarz HF receivers with the line-feed framed `F` / `I` sets (EK 895, EK 896,
  XK 2100) and the `*F` / `*I` set (XK 852).
- **`tentec_rx320.rs`**: Ten-Tec RX-320 and Pegasus TT-550 (the `rx320` and `tt550` dialects).
- **`uniden_digital.rs`**: Uniden BCD396T and BCD996T digital trunking scanners (`GLG` / `QSH`).
- **`serial_vendor.rs`**: the receive buffer, digit parsing and the dispatch from a description's
  `vendor.protocol` to its engine and twin, shared by the vendor serial engines (Ten-Tec, Drake, Racal and
  others); `VendorTwin` lets the conformance harness drive every one the same way.
- **`usb_family.rs`**: the dispatch for the `usb-device` family, from a description to its engine, its virtual
  twin and the device to look for on the bus (chapter [5](05-transports.md#usb-devices)).
- **`perseus_engine.rs`**: the Microtelecom Perseus as an engine; it encodes each operation as one control
  transfer for the `perseus` loader (known gap 11, chapter [13](13-roadmap.md)).
- **`barrett4100.rs`**: the Barrett 4100 HF transceiver (Hamlib's Beta-quality driver is the only source; no
  Barrett document was read).
- **`rshfiq.rs`**: the HobbyPCB RS-HFIQ transceiver (commands start with `*` and end with CR).
- **`easy.rs`**: the one-call constructors behind the prelude (`VirtualRig::spawn`, `spawn_virtual`);
  chapter [2](02-quick-start.md).

## What "agree" means here

The oracle (chapter [11](11-testing.md)) drives the virtual twin of a radio once through Hamlib's `rigctld`
and once through radioctl's engine and compares readings and final states. Two programs reading the same
documents can agree and both be wrong about a radio; the oracle finds typing and layout mistakes, not
facts about the hardware.
