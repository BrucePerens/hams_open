# Rig coverage plan

Decision (Bruce Perens, 2026-10-31): it is not realistic for us to try all of the supported hardware. We rely on
volunteers running tests or reporting bugs. Implement all of Hamlib's models, keep a list of which have been
verified, and make a best effort to make them work out of the box.

This plan splits the remaining work into four tiers. The counts are from [RIG_SUPPORT.md](RIG_SUPPORT.md), which is
generated from Hamlib 4.6.2's 313 models and the files in `rigs/` and is the place to look for current numbers.
Today 41 of 313 models have a description (46 files, because the four FlexRadio 6000-series files share Hamlib's one
"6xxx" model and the two ANAN files have no Hamlib model), and none is hardware-verified.

| Tier | Work | Models | Described | Left |
|---|---|---:|---:|---:|
| A | data only, on an existing engine | 135 | 37 | 98 |
| B | variants of an existing engine | 41 | 4 | 37 |
| C | a new engine per family | 83 | 0 | 83 |
| D | odd devices | 54 | 0 | 54 |

## Rules that apply to every tier

These are the rules the existing 46 descriptions already follow.

1. **Facts come from public manuals and protocol documentation**, with the document, edition and section recorded in
   the fact's `provenance`. Never copy code or tables from anywhere.
2. **Skip sources whose terms forbid copying** (as with Icom's CI-V guides and the Xiegu X6200 document); record in the
   fact that the source was skipped and why. Do not ask for permission, and do not use the skipped text.
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
  provenance. Icom and Xiegu documents forbid reuse: use Hamlib facts and `--dump-caps` and say so, as the shipped files do.
- Order inside the tier: models with many users first (Icom IC-7000/7100/7200/746/756 series, Kenwood TS-480/570/990/50,
  Yaesu FT-710/891/950/2000/FTDX-3000/5000, Elecraft K3/K3S/KX2/K4), then the rest by family, one pull request per family.
- 37 are done (the 18 earlier ones and 19 Icom CI-V models, see below). Done means a description exists, not that it is verified.
- Flex: the SmartSDR "slice A to H" models are the same network engine; one file covers them with the slice letter as a parameter.

### Icom CI-V: what is done and what is left (79 models in Hamlib's `icom_civ` family)

Done (28, incl. 5 Xiegu): IC-7300, 705, 7610, 9700 (first batch); IC-7000, 7100, 7200, 7410, 7600, 7700, 7760, 7800, 7850/7851,
9100, R8600, R75, R9500, 706, 706MkII, 706MkIIG, 718, 746, 746PRO (batch of 2026-10-31, generated from Hamlib 4.6.2 facts
and `rigctl --dump-caps`); Xiegu G90, X6100, X6200, X108G, X5105.

Left (51), with the reason each is not yet done:

| Group | Hamlib models (id) | What is needed |
|---|---|---|
| HF/VHF, 5-byte frequency, data only | IC-703 (3055), IC-756 (3026), 756PRO (3027), 756PROII (3047), 756PROIII (3057), IC-910 (3044), IC-2730 (3072), ID-5100 (3071), IC-78 (3045), IC-707 (3012), IC-729 (3017), IC-736 (3020), 737 (3021), 738 (3022), 775 (3030), 781 (3031), 970 (3035) | a description each; next batch |
| Older HF and VHF/UHF, 5-byte | IC-725 (3014), 726 (3015), 728 (3016), 751 (3024), 761 (3028), 765 (3029), 1275 (3002), 271 (3003), 275 (3004), 375 (3005), 471 (3006), 475 (3007), 575 (3008) | a description each |
| Four-byte frequency (Hamlib `civ_731_mode`) | IC-735 (3019), IC-820H (3032), IC-821H (3034) | engine change: 8-digit BCD frequency (`[civ]` field), virtual rig and transcript |
| Receivers | IC-R10 (3036), R20 (3058), R6 (3077), R30 (3080), R71 (3037), R72 (3038), R7000 (3040), R7100 (3041), R8500 (3042), R9000 (3043), RX7 (3069) | a description each; the handhelds (R6, R10, R20, R30) are to be checked for mode codes |
| D-STAR and handhelds | IC-92D (3065), ID-1 (3054), ID-31 (3083), ID-4100 (3082), ID-51 (3084), IC-F8101 (3086) | mode vocabulary (D-STAR modes are not in `Mode`), a description each |
| Microwave | IC-905 (3090) | engine change: frequencies above 9,999,999,999 Hz do not fit the 5-byte field |

The Main/Sub radios (IC-7600, 7800, 7850/7851, 7760; the IC-7610 and IC-9700 before them) have no model of command `07 C2`/`D0`/`D1`/`D2`
in the engine: the oracle records that as divergences. Fixing that is an engine task, to be done with a transcript from a real radio.

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
