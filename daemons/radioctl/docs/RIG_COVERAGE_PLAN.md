# Rig coverage plan

Decision (Bruce Perens, 2026-10-31): it is not realistic for us to try all of the supported hardware. We rely on
volunteers running tests or reporting bugs. Implement all of Hamlib's models, keep a list of which have been
verified, and make a best effort to make them work out of the box.

This plan splits the remaining work into four tiers. The counts are from [RIG_SUPPORT.md](RIG_SUPPORT.md), which is
generated from Hamlib 4.6.2's 313 models and the files in `rigs/` and is the place to look for current numbers.
Today 120 of 313 models have a description (125 files, because the four FlexRadio 6000-series files share Hamlib's one
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

Left (4), with the reason each is not yet done:

| Group | Hamlib models (id) | What is needed |
|---|---|---|
| IC-92D (3065) | Hamlib's `ic92d` model has no frequency or mode functions at all (only `get_info`), so a description could claim nothing but `identify`; waits for a manual | the model's own CI-V frequency commands |
| IC-F8101 (3086) | Hamlib's `icf8101` reads and sets frequency and mode with command `1A 34`/`35`/`36` and `1A 05`, not `03`/`05`/`04`/`06` (seen in the oracle frames); needs an engine variant for that dialect | a CI-V dialect in the engine |
| Four-byte frequency, VHF/UHF | IC-820H (3032), IC-821H (3034) | Hamlib's `civ_731_mode` sends 8 BCD digits of hertz, which cannot hold 145 or 435 MHz (Hamlib's `to_bcd` truncates); the radio most likely counts in another unit that no source read here states. Held back until a manual or a volunteer's capture says which; the engine field is ready |

The Main/Sub radios (IC-7600, 7800, 7850/7851, 7760; the IC-7610 and IC-9700 before them) have no model of command `07 C2`/`D0`/`D1`/`D2`
in the engine: the oracle records that as divergences. Fixing that is an engine task, to be done with a transcript from a real radio.

### Kenwood ASCII: what is done and what is left

Done (16, batch of 2026-10-31, first written from Hamlib 4.6.2 facts and `--dump-caps` because kenwood.com refuses automated downloads; the TS-480 and TS-870S were then checked against public copies of their manuals, see below): TS-50S, 140S, 450S, 480, 570D, 570S, 680S, 690S, 711, 790, 811, 850, 870S, 940S, 950S, 950SDX. Declared capabilities are those Hamlib implements; levels only for the TS-480.

Left, with the reason:

| Model (Hamlib id) | What is needed |
|---|---|
| R-5000 (2015) | Hamlib's IC-10 dialect with an `IF` of 32 characters; the TS-440S is done as `kenwood.dialect = "ts440"` (its IC-10 manual shows a 38-character `IF`, not Hamlib's 37) and the R-5000 needs a twin |
| TS-930 (2022) | Hamlib has no `ID` for it (the number is unknown), so the description cannot be written honestly; needs a volunteer's `ID;` answer |
| TS-990S (2039) | Main/Sub receivers with no VFO A/B in Hamlib's model and its own mode letters (C to H): needs a Main/Sub model in the engine. The PC command reference (May 2015) is downloaded (people.freebsd.org mirror) and ready to read |
| TRC-80 (2030) | a transceiver module for a commercial radio; Hamlib reads its frequency from `IF` (`kenwood_get_freq_if`), a different path from the other models; needs a guide or a volunteer |
| Levels, signal and meters on the older models (all but the TS-480) | the per-model AG/RG/PC/KS/SM forms. The TS-870S manual was read: AGnnn, RGnnn, SMnnnn, KS 001-100, PC 010-100, parameterless RU/RD (a TS-870S dialect of the engine, or per-description level ranges, would carry these). The TS-480's RG is 000-100 (pinned). Manuals for the rest are in `~/manuals/kenwood/` but their command tables are scanned images not yet read |
| The 10 TH/TM handhelds (`kenwood_hh_ascii`, Tier B) | a different command family (CR-terminated, no `IF`); not covered here |

### Yaesu newcat and Elecraft: what is done and what is left

Done (15, batch of 2026-10-31, from Yaesu's and Elecraft's public guides with sha256 per fact, Hamlib 4.6.2 as cross-check): Yaesu FT-450D, FT-450 (written from Hamlib facts; compared on 2026-10-31 with the FT-450 CAT Operation Reference Book: ID, 8-digit frequency, 27-character IF and modes agree; `read-power` added; MG, PC and clarifier forms still differ from the engine), FT-950, FT-2000, FTDX-1200, FTDX-3000, FTDX-5000, FTDX-9000, FT-891, FT-710, FTDX-101MP; Elecraft K3, K3S, KX2, K4. Engine change: `yaesu.frequency_digits` (8 for the older models). Source terms: none of the Yaesu or Elecraft documents forbids reuse of its facts; nothing was copied.

Left, with the reason:

| Model (Hamlib id) | What is needed |
|---|---|
| Yaesu FTDX-9000 Old (1050) | Hamlib's separate model for older firmware; its ID is not in the FTDX9000 guide. Needs a volunteer's `ID;` answer |
| Elecraft K2 (2021) | the K2 answers a shorter `IF`/`MD` set (K2 Owner's Manual appendix); needs a K2 dialect in `elecraft.rs` |
| Elecraft XG3 (2038) | a signal source, not a transceiver: no frequency/mode commands of the K3 family; needs its own engine or a note as "not a rig" |
| Levels on the FT-450D, FT-950, FT-2000, FTDX-5000, FTDX-9000 (MG and PC 0-255) and FT-891 (RG 0-30) | per-model level ranges in the Yaesu engine (the table is the FT-991A's) |
| Split on the FT-891 (`ST`), FT-710 and the dual-receiver Yaesus (`FT` selects a band) | `ST` and band-transmitter forms in the Yaesu engine |
| XIT/RIT on the FT-450, FT-891, FT-710 (`CF`, no `RT`/`XT`) | the `CF` clarifier command in the Yaesu engine |
| K4 levels, RIT/XIT, signal and split | the K4 forms of AG, RG, KS, MG, PC, RA, PA, RT/XT/RU/RD and SM differ from the K3 ones the Elecraft engine sends |

## Tier B: variants of an existing engine (41 models)

- Yaesu legacy five-byte CAT (31 models): the FT-817, 818, 857 and 897 are done. The rest (FT-747, 767, 840, 890, 900,
  920, 1000D, 1000MP, FT-100, FT-450 family outliers and others) differ in block layout, mode bytes and which commands
  exist. Each needs its own parameters in `yaesu_legacy` and a transcript test.
- Kenwood hand-helds and mobiles (TH-D7, D72, D74, F6, F7, G71, TM-D700, D710, V7, V71; 10 models): ASCII with CR and
  different command sets from the HF Kenwoods. A dialect in the Kenwood engine, not a new engine.
- Source: public manuals and protocol notes; Hamlib as oracle.

## Tier C: new engines (83 models)

Each engine is a pull request with its protocol document recorded, a virtual twin, a transcript fixture, and
descriptions for every model in that family. Order by number of models and by how many amateurs own them:

1. Ten-Tec (13: Argonaut, Jupiter, Pegasus, Omni, Orion, Eagle, RX-320 and others): one binary and ASCII protocol family.
2. AOR (11 receivers) and Uniden/Radio Shack (8) and Icom PCR (4): ASCII scanners and receivers.
3. Rohde and Schwarz (6), JRC (5), Winradio (8 through their vendor interface), Barrett (3), Alinco (2), Racal (2),
   Drake (2), CODAN (2), Skanti (2), Optoelectronics (2) and single models (Lowe, Watkins-Johnson, Kachina, Micom,
   Lab599, Hilberling, Guohe, Vertex VX-1700).
4. Icom marine (4): CI-V-like variants for IC-M700PRO, M710, M802, M803.

Skip any model whose only documentation forbids copying; mark it "needs engine, no usable source" in RIG_SUPPORT.md.

## Tier D: odd devices (54 models)

SDR boards and dongles on USB or serial (SoftRock, FUNcube, Peaberry, FiFi-SDR, Elektor, QRP Labs, Malachite,
SDRPlay), network drivers (FLRig, TRXManager, N3FJP, Airspy, Quisk, HiQSDR, DttSP), parallel-port devices (SDR-1000,
DDS-60), Video4Linux radios, Hamlib's dummy and NET rigctl entries, and Hamlib's own virtual rigs. Many are not rigs at all.

- Where radioctl already has a native equivalent (HPSDR over UDP, the FlexRadio API, the dummy rig) map the Hamlib model to it.
- Network drivers that only forward to another program (FLRig, NET rigctl) get a pass-through transport, not an engine.
- Parallel-port and Video4Linux hardware: document as unsupported unless a volunteer asks for it and tests it.
- Done last; most value is in HPSDR and SDR dongles on Linux.

## Recommended order

1. Finish the IC-7300 bench day; it sets the model for the first `hardware-verified` entry in `rigs/reports/verified.toml`.
2. Publish the self-test kit and ask for volunteers on the most common radios already described (Tier A: 37 done).
3. Tier A, family by family: Icom CI-V, Kenwood, Yaesu NewCAT, Elecraft, Flex.
4. Tier B, Yaesu legacy first, then the Kenwood hand-helds.
5. Tier C in the order above; Ten-Tec first.
6. Tier D, starting with the entries that map to existing engines.

Work items are in `hams_com/night_shift_todo/medium/` (`radioctl-rig-coverage-tier-*.md`).
