# Rig coverage plan

Decision (Bruce Perens, 2026-10-31): it is not realistic for us to try all of the supported hardware. We rely on
volunteers running tests or reporting bugs. Implement all of Hamlib's models, keep a list of which have been
verified, and make a best effort to make them work out of the box.

This plan splits the remaining work into four tiers. The counts are from [RIG_SUPPORT.md](RIG_SUPPORT.md), which is
generated from Hamlib 4.6.2's 313 models and the files in `rigs/` and is the place to look for current numbers.
Today 121 of 313 models have a description (126 files, because the four FlexRadio 6000-series files share Hamlib's one
"6xxx" model and the two ANAN files have no Hamlib model), and none is hardware-verified.

| Tier | Work | Models | Described | Left |
|---|---|---:|---:|---:|
| A | data only, on an existing engine | 135 | 116 | 19 |
| B | variants of an existing engine | 41 | 4 | 37 |
| C | a new engine per family | 83 | 0 | 83 |
| D | odd devices | 54 | 0 | 54 |

## Rules that apply to every tier

These are the rules the existing 46 descriptions already follow.

1. **Facts come from public manuals and protocol documentation**, with the document, edition and section recorded in
   the fact's `provenance`. Never copy code or tables from anywhere.
2. **Skip sources whose terms forbid copying** (as with the Xiegu X6200 document); record in the
   fact that the source was skipped and why. Do not ask for permission, and do not use the skipped text.
   Exception, by Bruce Perens' decision of 2026-10-31: the Icom manuals (their terms are unreasonable and would not
   survive a court) and any manual with redistribution provisions may be downloaded and read for facts. They are
   stored outside every git repository (`~/manuals/icom/`, sha256 in `SHA256SUMS`), only publicly reachable files are
   fetched (robots.txt honoured: Icom's `/api/` download endpoint is excluded, so the archive.org mirrors are used),
   and they are never committed, published or redistributed; facts are written in our own words with the document,
   page, URL and sha256 in the fact.
3. **Hamlib is an oracle and a fact check, not a source of code.** Read its source for facts only (an address, a
   command number), name the file and function in the `provenance`, and cross-check with the `hamlib-oracle`
   differential tests where the model's virtual twin allows. `rigctl -l` and `--dump-caps` output are program output.
4. **Everything is `unverified` until a volunteer confirms it** with [VOLUNTEER_TESTING.md](VOLUNTEER_TESTING.md).
   Moving up a level is the maintainer's act, in the rig file and `rigs/reports/verified.toml`. `manual-checked` and
   `transcript-checked` are for comparison with a document or a recorded transcript, never for "looks right".
5. **The transmit gate is closed by default.** PTT and CW commands for an unverified rig are encoded where the
   protocol has them, and documented as unverified. Nothing in the self-test or in the descriptions opens the gate.
6. **Same safety for every rig**: one transmit-safety baseline, no per-model exemptions.
7. Every new Rust function carries its `[@ANCHOR]` and a test citing it; every new rig file passes the conformance
   suite (`radioctl_protocols::conformance`) and gets a virtual twin that the engine tests drive.

## Tier A: data only on an existing engine (135 models)

Engines exist: Icom CI-V (79 models; includes the Xiegu CI-V radios), Kenwood ASCII (25), Yaesu NewCAT (15),
Elecraft (7), FlexRadio (9). The work is a `rigs/*.toml` per model with its addresses, mode codes, ranges and
capabilities, plus an engine fix when a model differs.

- Source: the vendor's public manual or CAT reference where its terms allow, else Hamlib facts with
  provenance. Xiegu documents forbid reuse: use Hamlib facts and `--dump-caps` and say so, as the shipped files do. Icom's manuals are read for facts (rule 2).
- Order inside the tier: models with many users first (Icom IC-7000/7100/7200/746/756 series, Kenwood TS-480/570/990/50,
  Yaesu FT-710/891/950/2000/FTDX-3000/5000, Elecraft K3/K3S/KX2/K4), then the rest by family, one pull request per family.
- 116 are done (the 18 earlier ones, 66 Icom CI-V models, 15 Yaesu newcat and Elecraft models and 16 Kenwood ASCII models, see below). Done means a description exists, not that it is verified.
- Flex: the SmartSDR "slice A to H" models are the same network engine; one file covers them with the slice letter as a parameter.

### Icom CI-V: what is done and what is left (79 models in Hamlib's `icom_civ` family)

Done (75, incl. 5 Xiegu): IC-7300, 705, 7610, 9700 (first batch); IC-7000, 7100, 7200, 7410, 7600, 7700, 7760, 7800, 7850/7851,
9100, R8600, R75, R9500, 706, 706MkII, 706MkIIG, 718, 746, 746PRO (batch of 2026-10-31, generated from Hamlib 4.6.2 facts
and `rigctl --dump-caps`, then re-read against Icom's manuals under Bruce's decision: 15 of the 19 have a readable manual, see README); Xiegu G90, X6100, X6200, X108G, X5105; third batch of 2026-10-31 (16, Hamlib facts only, no manual read): IC-703, 756, 756PRO, 756PROII, 756PROIII, 910, 2730, 78, 707, 729, 736, 737, 738, 775, 781, 970; and the IC-735 and IC-905 (engine change `civ.frequency_bytes`: 4 for Hamlib's `civ_731_mode`, 6 for the IC-905, whose field is 5 bytes up to 5.85 GHz and 6 above, as in Hamlib's `icom_set_freq`). Fourth batch of 2026-10-31 (20, Hamlib facts only, no manual read): IC-725, 726, 728, 751, 761, 765, 1275, 271, 275, 375, 471, 475, 575, R71, R72, R7000, R7100, R8500, R9000, RX7. Fifth batch of 2026-10-31 (9, Hamlib facts only, no manual read): the D-STAR models ID-5100, ID-31, ID-4100, ID-51 (new mode `d-star`, code 17, Hamlib name D-STAR, bit 24), ID-1 and the handheld receivers R6, R10, R20, R30. Engine change: `levels.absent` (per-description list of the plain `14` levels a model lacks; the IC-706MkIIG now lists all six and the oracle pin is gone).

All 79 have a description. The last four (2026-10-31, Hamlib facts; none verified):

- IC-820H (3032) and IC-821H (3034): Hamlib sets its 8-digit `civ_731_mode`, which cannot hold 145 or 435 MHz. The general Icom CI-V Reference Manual (1993) gives a fixed five-byte field to every radio but the IC-735/IC-73S, so both files use five bytes; Hamlib disagrees (pinned in `rigctld_oracle.rs`).
- IC-F8101 (3086): `civ.command_set = "f8101"`: frequency set `1A 35`, mode read `1A 34`, mode set `1A 36` (Hamlib `icf8101.c`); Hamlib split (`1A 05`) and PTT (`1A 00`) are not implemented.
- IC-92D (3065): Hamlib has no frequency or mode function for it, so its description claims only `identify` (address 0x01, 38400 bit/s). Its own CI-V commands wait for a manual or a volunteer.

The Main/Sub radios (IC-7600, 7800, 7850/7851, 7760; the IC-7610 and IC-9700 before them) have no model of command `07 C2`/`D0`/`D1`/`D2`
in the engine: the oracle records that as divergences. Fixing that is an engine task, to be done with a transcript from a real radio.

### Kenwood ASCII: what is done and what is left

Done (16, batch of 2026-10-31, first written from Hamlib 4.6.2 facts and `--dump-caps` because kenwood.com refuses automated downloads; the TS-480 and TS-870S were then checked against public copies of their manuals, see below): TS-50S, 140S, 450S, 480, 570D, 570S, 680S, 690S, 711, 790, 811, 850, 870S, 940S, 950S, 950SDX. Declared capabilities are those Hamlib implements; levels only for the TS-480.

Left, with the reason:

| Model (Hamlib id) | What is needed |
|---|---|
| (done 2026-10-31, Hamlib facts, unverified) | R-5000 (`kenwood.dialect = "r5000"`: IC-10 set, an IF of 30 characters after IF, five fewer than the TS-440S: the position of the missing field is inferred), TRC-80 (`"trc80"`: the frequency is read from IF; Hamlib reads 12 characters after IF so it disagrees with the virtual rig, pinned), TS-930 (classic set, no known ID: `model_id = 0` is a placeholder and identify is not claimed), Elecraft K2 (K3 command forms for frequency, mode and TX/RX, ID017 from the KIO2 manual page), Yaesu FTDX-9000 Old (same data as the FTDX-9000; Hamlib's ID check rejects 0101, pinned) |
| TS-990S (2039) | Described from its PC command reference (`kenwood.dialect = "ts990"`: main band only, mode as `OM0`, no split, PTT read or levels). Open: the sub receiver as a second rig, the levels (band digits, other ranges) and the LAN stream (`##CN`, `##ID`), unverified |
| Levels, signal and meters on the older models (all but the TS-480) | the per-model AG/RG/PC/KS/SM forms. The TS-870S manual was read: AGnnn, RGnnn, SMnnnn, KS 001-100, PC 010-100, parameterless RU/RD (a TS-870S dialect of the engine, or per-description level ranges, would carry these). The TS-480's RG is 000-100 (pinned). Manuals for the rest are in `~/manuals/kenwood/` but their command tables are scanned images not yet read |
| The 10 TH/TM handhelds (`kenwood_hh_ascii`, Tier B) | done: engine `kenwood_hh` (CR-terminated, no `IF`; dialects `th`, `tm710`, `th72`, `th74`), 10 files from Hamlib facts, all unverified. Open: the `TC 1` PC-control entry named in the work brief (in none of the sources read, so not sent), band switching (`BC`), split, levels, memories, narrow FM and DV/DR modes |

### Yaesu newcat and Elecraft: what is done and what is left

Done (15, batch of 2026-10-31, from Yaesu's and Elecraft's public guides with sha256 per fact, Hamlib 4.6.2 as cross-check): Yaesu FT-450D, FT-450 (written from Hamlib facts; compared on 2026-10-31 with the FT-450 CAT Operation Reference Book: ID, 8-digit frequency, 27-character IF and modes agree; `read-power` added; MG, PC and clarifier forms still differ from the engine), FT-950, FT-2000, FTDX-1200, FTDX-3000, FTDX-5000, FTDX-9000, FT-891, FT-710, FTDX-101MP; Elecraft K3, K3S, KX2, K4. Engine change: `yaesu.frequency_digits` (8 for the older models). Source terms: none of the Yaesu or Elecraft documents forbids reuse of its facts; nothing was copied.

Left, with the reason:

| Model (Hamlib id) | What is needed |
|---|---|
| Elecraft XG3 (2038) | a signal source with its own single-letter command set (XG3 owner's manual, page 18 on): needs its own engine |
| Levels on the FT-450D, FT-950, FT-2000, FTDX-5000, FTDX-9000 (MG and PC 0-255) and FT-891 (RG 0-30) | per-model level ranges in the Yaesu engine (the table is the FT-991A's) |
| Split on the FT-891 (`ST`), FT-710 and the dual-receiver Yaesus (`FT` selects a band) | `ST` and band-transmitter forms in the Yaesu engine |
| XIT/RIT on the FT-450, FT-891, FT-710 (`CF`, no `RT`/`XT`) | the `CF` clarifier command in the Yaesu engine |
| K4 levels, RIT/XIT, signal and split | the K4 forms of AG, RG, KS, MG, PC, RA, PA, RT/XT/RU/RD and SM differ from the K3 ones the Elecraft engine sends |

### Vendor serial engines (Ten-Tec, Drake, Alinco, Racal): what is done and what is left

One description family, `serial-vendor`, carries every small-maker serial protocol (`[vendor] protocol = ...`, `dialect = ...`), each with its own engine file (`tentec.rs`, `drake.rs`, `alinco.rs`, `racal.rs`) and a virtual twin in the same file (`serial_vendor.rs` holds the shared receive buffer and dispatch).

Done (7, 2026-10-31, Hamlib 4.6.2 facts only, no vendor document read): Orion TT-565/566 and Eagle TT-599 (dialect `orion`, ASCII), Argonaut V TT-516 (`argonaut`), Jupiter TT-538 (`jupiter`), Omni VII TT-588 (`omnivii`); the Omni VI Plus and Delta II speak CI-V (Hamlib reuses its Icom backend) and run on the Icom engine. Also done (2026-10-31, Hamlib facts only): Drake R-8A and R-8B (`drake.rs`, dialect `r8`), Alinco DX-77 and DX-SR8 (`alinco.rs`, dialects `dx77` and `dxsr8`), Racal RA6790/GM (`racal.rs`, dialect `ra6790`); QRP Labs QCX/QDX and QMX (Kenwood TS-480 emulation, descriptions only on the classic Kenwood engine; the ID answer 20 and the IF length are assumptions).

Left, with the reason:

| Model (Hamlib id) | What is needed |
|---|---|
| RX-320 (16003), RX-350 (16005) | DSP receivers tuned by a computed tuning factor and BFO words; needs its own dialect and a tolerance-based round-trip test |
| RX-340 (16004), RX-331 (16012) | `$`-prefixed ASCII receiver protocol |
| TT-550 Pegasus (16001), TT-585 Paragon (16009) | each has its own binary protocol |
| Skanti TRP8000, TRP 8255 S R (14002, 14004), Barrett 950/2050/4050 (32001-32003), CODAN Envoy/NGT (34001, 34002), Guohe Q900/PMR-171 (1051, 1052) | not started: Barrett replies are framed by XON/XOFF bytes, Guohe has its own binary protocol (`pmr171.c`); each needs a protocol read and an engine file |
| Elektor 3/04 (25001), Dorji DRA818U/V (31001, 31002), MDS 4710/9710 (36001, 36002) | not started; kit and OEM modules with their own command sets |
| Racal RA3702 (11005) | the RA37xx packet protocol (LF-started frames with a receiver number in the header, `QF`/`QM` queries); not read yet |
| Hamlib oracle batch for the vendor serial files | the oracle harness drives `VirtualRig`; it needs a generic twin hook |

## Tier B: variants of an existing engine (41 models)

- Yaesu legacy five-byte CAT (31 models plus the Vertex VX-1700): all described (31 + 1, all unverified). The FT-817 family (9 models) uses the engine's built-in opcodes. The other 22, and the VX-1700 (Hamlib's `yaesu_other` backend, but the same five-byte block protocol as the FT-890), are data in the description (`yaesu_legacy.set_frequency`, `set_mode`, `ptt_on/off`, `split_on/off`, `[[yaesu_legacy.reads]]` status reads with field offsets and frequency forms, `[[yaesu_legacy.status_modes]]`), written from the manuals' CAT chapters where a copy was found (FT-747GX, FT-757GXII, FT-767GX, FT-840, FT-890, FT-900, FT-920, FT-990, FT-1000/1000D, FT-1000MP MARK-V and Field, FT-100, FRG-100, FRG-8800, VR-5000) and from Hamlib facts otherwise (FT-757GX original, FT-600, FT-980, FRG-9600, VX-1700). Open and unverified, each pinned in `yaesu_legacy.rs` tests or noted in the file: FT-747 block length (345 manual, 344 Hamlib) and displayed-frequency offset; FT-890 split parameter polarity; FT-990 operating-data length (manual 16, Hamlib 32); FT-920 status frequency/mode encodings (not in the manual); FT-1000MP status frequency unit (0.625 Hz per Hamlib, undecodable in the manual); FT-100 status length (manual 16, Hamlib 32); VR-5000 25 kHz step code (34 manual, 35 Hamlib); FRG-100 update length and ID bytes. Not offered: the FT-767GX status reads (they need an acknowledge round trip the engine does not do), the S-meter and power meters of the block family (opcode F7). No CAT pages could be read for the FT-600, FT-980 and FRG-9600 (no public copy has them) or the VX-1700 (its CAT manual's only public copy is on a site that disallows AI agents, so it was not read); those four are Hamlib facts only.
- Kenwood hand-helds and mobiles (TH-D7, D72, D74, F6, F7, G71, TM-D700, D710, V7, V71; 10 models): ASCII with CR and
  different command sets from the HF Kenwoods. A dialect in the Kenwood engine, not a new engine.
- Source: public manuals and protocol notes; Hamlib as oracle.

## Tier C: new engines (83 models)

Each engine is a pull request with its protocol document recorded, a virtual twin, a transcript fixture, and
descriptions for every model in that family. Order by number of models and by how many amateurs own them:

1. Ten-Tec (13: Argonaut, Jupiter, Pegasus, Omni, Orion, Eagle, RX-320 and others): one binary and ASCII protocol family.
2. AOR (11 receivers; the AR8000, AR8200, AR8600, AR5000, AR5000A, AR2700 and SR2200 are done in `aor.rs`; the AR3000A, AR3030 and the binary AR7030 and AR7030 Plus have command sets of their own and are still open) and Uniden/Radio Shack (8) and Icom PCR (4): ASCII scanners and receivers. The Uniden BC245XLT, BC250D, BC780XLT, BC895XLT and BC898T are done in `uniden.rs`, the Icom PCR four in `icom_pcr.rs`; the BCD-396T and BCD-996T are done in `uniden_digital.rs` from Uniden's remote command specification (Hamlib has only stubs for them, so the oracle test pins that every Hamlib step is refused).
3. Rohde and Schwarz (6; the ESMC and EB200 are done in `rs.rs`, the EK895, EK896, XK852 and XK2100 in `rs_hf.rs`: the EK895/6 from the operator manual's data-link chapter, the two XK from Hamlib only because no manual was found; their PTT is not encoded), JRC (5; done in `jrc.rs`), Winradio (8 through their vendor interface), Barrett (3), Alinco (2), Racal (2),
   Drake (2), CODAN (2), Skanti (2), Optoelectronics (2; done: the OptoScan boards speak Icom CI-V at address 0x80, so they are plain `icom-civ` descriptions) and single models (Lowe, Watkins-Johnson, Kachina, Micom,
   Lab599, Hilberling, Guohe, Vertex VX-1700).
4. Icom marine (4): done in `icom_marine.rs`. They are not CI-V-like: Hamlib speaks NMEA-style `$PICOA` sentences to them.

Winradio (8 models, Hamlib ids 15001 to 15007 and 15009) is **not applicable**: Hamlib reaches these radios only through
the vendor's driver (the Linux `/dev/winradio` ioctl interface of the kernel driver, or the vendor DLL on Windows;
the G313 through its vendor API). There is no serial, USB or network protocol document a program can speak, so radioctl
has nothing to implement. `tools/rig_support_matrix.py` records each as `not applicable` with this reason. If Winradio
ever publishes the USB protocol, or a volunteer documents it, they move to Tier C.

Skip any model whose only documentation forbids copying; mark it "needs engine, no usable source" in RIG_SUPPORT.md.

## Tier D: odd devices (54 models, 19 of them not applicable)

SDR boards and dongles on USB or serial (SoftRock, FUNcube, Peaberry, FiFi-SDR, Elektor, QRP Labs, Malachite,
SDRPlay), network drivers (FLRig, TRXManager, N3FJP, Airspy, Quisk, HiQSDR, DttSP), parallel-port devices (SDR-1000,
DDS-60), Video4Linux radios, Hamlib's dummy and NET rigctl entries, and Hamlib's own virtual rigs. Many are not rigs at all.

- Where radioctl already has a native equivalent (HPSDR over UDP, the FlexRadio API, the dummy rig) map the Hamlib model to it.
- Network drivers that only forward to another program (FLRig, NET rigctl) get a pass-through transport, not an engine.
- Parallel-port and Video4Linux hardware: document as unsupported unless a volunteer asks for it and tests it.
- Done last; most value is in HPSDR and SDR dongles on Linux.

Decisions (2026-10-31, Bruce): Hamlib entries that are not real radios are **not applicable** and are not implemented
(radioctl has better virtual rigs of its own); `RIG_SUPPORT.md` counts them separately so that parity means every real
radio described. Classified so far (`NOT_APPLICABLE` in `tools/rig_support_matrix.py` holds the one-line reason of each):

- Simulators: Hamlib Dummy (1) and Dummy No VFO (6).
- Clients of another rig server or program: NET rigctl (2), FLRig (4), TRXManager (5), ACLog (8), SDR# and gpredict (9).
- Software SDR or DSP programs: Quisk (10), DttSP IPC (23003) and UDP (23004); PowerSDR (2048), Thetis (2054), SDRConsole
  (2056), SDRUno (2051) and PiHPSDR (2040) are programs that imitate a Kenwood CAT server, so a program speaking
  TS-2000 or TS-480 CAT can already use those descriptions.
- Kernel tuner cards: Video4Linux (26001) and Video4Linux2 (26002), reached only through the kernel's ioctl interface.
- USB boards and dongles (10 described, no engine): the Si570 kits (SoftRock AVR-USB, KTH PIC-USB, Peaberry V1 and V2, FA-SDR),
  FiFi-SDR, the two FUNcube dongles, Elektor SDR-USB and the Digital World Traveller have `family = "usb-device"` descriptions
  (USB ids, strings, multiplier; Hamlib 4.6.2 facts) but radioctl has no USB control or HID transport, so `engine_for`
  refuses them. The transport and the per-protocol engines are the to-do `radioctl-usb-hid-transport` (hams_com `night_shift_todo/medium`).
- Kenwood CAT emulations (done, on the classic Kenwood engine): (tr)uSDX (2055), Malachite DSP (2049), BG2FX FX-4 (2053),
  ELAD FDM-DUO (33001); the QRP Labs QCX/QDX and QMX went in with the Kenwood batch.
- Left in Tier D, each needing an engine or a transport of its own: Microtelecom Perseus (3074, a vendor USB library with
  firmware load) and N2ADR HiQSDR (25014, UDP control packets): both go with the USB/network transport to-do; SigFox Transfox
  (2032, its own commands over the Kenwood framing); M0NKA mcHF (1045, the FT-847 five-byte CAT in `yaesu_legacy`, owned by the
  Yaesu legacy batch); and the serial kits and small makers: ADAT ADT-200A (29001), AnyTone D578A (37001), Commradio CTX-10
  (39001), Dorji DRA818U and V (31001, 31002), GOMSPACE GS100 (35001), HobbyPCB RS-HFIQ (25019), MDS 4710 and 9710 (36001,
  36002), Philips/Simoco PRM8060 (28001), RFT EKD-500 (24001), SAT-Schneider DRT1 (25002), TAPR DSP-10 (22001) and Elektor 3/04 (25001).
- Measuring instrument: the mRS miniVNA (25008) is a vector network analyser that Hamlib lists as a tuner.
- Parallel-port devices: FlexRadio SDR-1000 (23001) and the AmQRP DDS-60 (25006); radioctl has no parallel-port transport.

## Recommended order

1. Finish the IC-7300 bench day; it sets the model for the first `hardware-verified` entry in `rigs/reports/verified.toml`.
2. Publish the self-test kit and ask for volunteers on the most common radios already described (Tier A: 37 done).
3. Tier A, family by family: Icom CI-V, Kenwood, Yaesu NewCAT, Elecraft, Flex.
4. Tier B, Yaesu legacy first, then the Kenwood hand-helds.
5. Tier C in the order above; Ten-Tec first.
6. Tier D, starting with the entries that map to existing engines.

Work items are in `hams_com/night_shift_todo/medium/` (`radioctl-rig-coverage-tier-*.md`).
