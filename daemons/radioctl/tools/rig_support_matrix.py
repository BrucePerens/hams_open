#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Writes docs/RIG_SUPPORT.md: every Hamlib model and where radioctl stands on it.

Inputs, all in this repository:

* data/hamlib-<version>/capability_matrix.json (program output of `rigctl`, see
  hamlib_caps_matrix.py): manufacturer, model, Hamlib id and the protocol family hint;
* rigs/*.toml: the rig descriptions. The highest `verified` state among a description's facts is
  its verification level; the state itself is never stored anywhere else;
* rigs/reports/verified.toml: human-edited reports of hardware tests (who reported). Its entries are
  checked against the facts so the two cannot disagree.

Mapping from a description to its Hamlib model ids is the explicit table HAMLIB_IDS below: no fuzzy
name matching, so a new description fails the test until its row is added.

Usage:
    rig_support_matrix.py            write docs/RIG_SUPPORT.md
    rig_support_matrix.py --check    exit 1 if the committed file differs from a fresh render
"""
from __future__ import annotations

import argparse
import json
import sys
import tomllib
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HAMLIB_DIR = ROOT / "data" / "hamlib-4.6.2"
RIGS_DIR = ROOT / "rigs"
OUT_PATH = ROOT / "docs" / "RIG_SUPPORT.md"

LEVELS = ["unverified", "manual-checked", "transcript-checked", "hardware-verified"]

# Description id -> Hamlib model ids it covers. Descriptions with no Hamlib model use [].
HAMLIB_IDS: dict[str, list[int]] = {
    "optoscan-os456": [3053],
    "optoscan-os535": [3052],
    "rs-eb200": [27002],
    "rs-ek895": [27004],
    "rs-ek896": [27004],
    "rs-xk2100": [27003],
    "rs-xk852": [27005],
    "rs-esmc": [27001],
    "icom-ic-m700pro": [30001],
    "icom-ic-m710": [30003],
    "icom-ic-m802": [30002],
    "icom-ic-m803": [30004],
    "icom-ic-pcr100": [4002],
    "icom-ic-pcr1000": [4001],
    "icom-ic-pcr1500": [4003],
    "icom-ic-pcr2500": [4004],
    "jrc-jst-145": [6001],
    "jrc-jst-245": [6002],
    "jrc-nrd-525": [6005],
    "jrc-nrd-535d": [6006],
    "jrc-nrd-545": [6007],
    "uniden-bc245xlt": [8002],
    "uniden-bc250d": [8006],
    "uniden-bc780xlt": [8001],
    "uniden-bc895xlt": [8003],
    "uniden-bc898t": [8012],
    "radioshack-pro-2052": [8004],
    "uniden-bcd396t": [8010],
    "uniden-bcd996t": [8011],
    "aor-ar2700": [5008],
    "aor-ar3000a": [5006],
    "aor-ar3030": [5005],
    "aor-ar7030": [5003],
    "aor-ar7030p": [5015],
    "aor-ar5000": [5004],
    "aor-ar5000a": [5014],
    "aor-ar8000": [5002],
    "aor-ar8200": [5001],
    "aor-ar8600": [5013],
    "aor-sr2200": [5016],
    "anan-10": [],
    "anan-100": [],
    "alinco-dx-77": [17001],
    "alinco-dx-sr8": [17002],
    "drake-r-8a": [9002],
    "drake-r-8b": [9003],
    "elecraft-kx3": [2045],
    "flex-6300": [2036],
    "flex-smartsdr-slice-a": [23005],
    "flex-smartsdr-slice-b": [23006],
    "flex-smartsdr-slice-c": [23007],
    "flex-smartsdr-slice-d": [23008],
    "flex-smartsdr-slice-e": [23009],
    "flex-smartsdr-slice-f": [23010],
    "flex-smartsdr-slice-g": [23011],
    "flex-smartsdr-slice-h": [23012],
    "flex-6400": [2036],
    "elecraft-k3": [2029],
    "elecraft-k3s": [2043],
    "elecraft-k4": [2047],
    "elecraft-kx2": [2044],
    "yaesu-ft-2000": [1029],
    "yaesu-ft-450": [1027],
    "yaesu-ft-450d": [1046],
    "yaesu-ft-710": [1049],
    "yaesu-ft-891": [1036],
    "yaesu-ft-950": [1028],
    "yaesu-ftdx-101mp": [1044],
    "yaesu-ftdx-1200": [1034],
    "yaesu-ftdx-3000": [1037],
    "yaesu-ftdx-5000": [1032],
    "yaesu-ftdx-9000": [1030],
    "flex-6600": [2036],
    "flex-6700": [2036],
    "icom-ic-7000": [3060],
    "icom-ic-705": [3085],
    "icom-ic-706": [3009],
    "icom-ic-706mkii": [3010],
    "icom-ic-706mkiig": [3011],
    "icom-ic-7100": [3070],
    "icom-ic-718": [3013],
    "icom-ic-7200": [3061],
    "icom-ic-7300": [3073],
    "icom-ic-7410": [3067],
    "icom-ic-746": [3023],
    "icom-ic-746pro": [3046],
    "icom-ic-7600": [3063],
    "icom-ic-7610": [3078],
    "icom-ic-7700": [3062],
    "icom-ic-7760": [3092],
    "icom-ic-7800": [3056],
    "icom-ic-7851": [3075],
    "icom-ic-9100": [3068],
    "icom-ic-9700": [3081],
    "icom-id-5100": [3071],
    "icom-id-1": [3054],
    "icom-id-31": [3083],
    "icom-id-4100": [3082],
    "icom-id-51": [3084],
    "icom-ic-r6": [3077],
    "icom-ic-r10": [3036],
    "icom-ic-r20": [3058],
    "icom-ic-r30": [3080],
    "icom-ic-820h": [3032],
    "icom-ic-821h": [3034],
    "icom-ic-92d": [3065],
    "icom-ic-f8101": [3086],
    "icom-ic-725": [3014],
    "icom-ic-726": [3015],
    "icom-ic-728": [3016],
    "icom-ic-751": [3024],
    "icom-ic-761": [3028],
    "icom-ic-765": [3029],
    "icom-ic-1275": [3002],
    "icom-ic-271": [3003],
    "icom-ic-275": [3004],
    "icom-ic-375": [3005],
    "icom-ic-471": [3006],
    "icom-ic-475": [3007],
    "icom-ic-575": [3008],
    "icom-ic-r71": [3037],
    "icom-ic-r72": [3038],
    "icom-ic-r7000": [3040],
    "icom-ic-r7100": [3041],
    "icom-ic-r8500": [3042],
    "icom-ic-r9000": [3043],
    "icom-ic-rx7": [3069],
    "icom-ic-735": [3019],
    "icom-ic-905": [3090],
    "icom-ic-703": [3055],
    "icom-ic-756": [3026],
    "icom-ic-756pro": [3027],
    "icom-ic-756proii": [3047],
    "icom-ic-756proiii": [3057],
    "icom-ic-910": [3044],
    "icom-ic-2730": [3072],
    "icom-ic-78": [3045],
    "icom-ic-707": [3012],
    "icom-ic-729": [3017],
    "icom-ic-736": [3020],
    "icom-ic-737": [3021],
    "icom-ic-738": [3022],
    "icom-ic-775": [3030],
    "icom-ic-781": [3031],
    "icom-ic-970": [3035],
    "icom-ic-r75": [3039],
    "icom-ic-r8600": [3079],
    "icom-ic-r9500": [3066],
    "kenwood-ts-50s": [2001],
    "kenwood-ts-140s": [2025],
    "kenwood-ts-440s": [2002],
    "kenwood-ts-450s": [2003],
    "kenwood-ts-480": [2028],
    "kenwood-ts-570d": [2004],
    "kenwood-ts-570s": [2016],
    "kenwood-ts-680s": [2024],
    "kenwood-ts-690s": [2005],
    "kenwood-ts-711": [2006],
    "kenwood-ts-790": [2007],
    "kenwood-ts-811": [2008],
    "kenwood-ts-850": [2009],
    "kenwood-ts-870s": [2010],
    "kenwood-ts-940s": [2011],
    "kenwood-ts-950s": [2012],
    "kenwood-ts-950sdx": [2013],
    "kenwood-r-5000": [2015],
    "kenwood-trc-80": [2030],
    "kenwood-ts-930": [2022],
    "elecraft-k2": [2021],
    "yaesu-ftdx-9000-old": [1050],
    "kenwood-ts-2000": [2014],
    "kenwood-ts-590s": [2031],
    "kenwood-ts-590sg": [2037],
    "kenwood-ts-990s": [2039],
    "kenwood-th-d7a": [2017],
    "kenwood-th-f6a": [2019],
    "kenwood-th-f7e": [2020],
    "kenwood-th-g71": [2023],
    "kenwood-tm-d700": [2026],
    "kenwood-tm-v7": [2027],
    "kenwood-th-d72a": [2033],
    "kenwood-tm-d710": [2034],
    "kenwood-tm-v71": [2035],
    "kenwood-th-d74": [2042],
    "kenwood-ts-890s": [2041],
    "xiegu-g90": [3088],
    "xiegu-x108g": [3076],
    "xiegu-x5105": [3089],
    "xiegu-x6100": [3087],
    "xiegu-x6200": [3091],
    "qrplabs-qcx-qdx": [2052],
    "dl2man-trusdx": [2055],
    "malachite-dsp": [2049],
    "bg2fx-fx4": [2053],
    "elad-fdm-duo": [33001],
    "qrplabs-qmx": [2057],
    "hilberling-pt-8000a": [2046],
    "lab599-tx-500": [2050],
    "racal-ra6790-gm": [11003],
    "adat-adt-200a": [29001],
    "commradio-ctx-10": [39001],
    "gomspace-gs100": [35001],
    "hobbypcb-rs-hfiq": [25019],
    "m0nka-mchf-qrp": [1045],
    "philips-prm8060": [28001],
    "rft-ekd-500": [24001],
    "barrett-2050": [32001],
    "barrett-4050": [32003],
    "barrett-950": [32002],
    "codan-envoy": [34001],
    "codan-ngt": [34002],
    "dorji-dra818u": [31002],
    "dorji-dra818v": [31001],
    "guohe-pmr-171": [1052],
    "guohe-q900": [1051],
    "kachina-505dsp": [18001],
    "lowe-hf-235": [10004],
    "racal-ra3702": [11005],
    "skanti-trp-8255-s-r": [14004],
    "skanti-trp8000": [14002],
    "tentec-rx-320": [16003],
    "tentec-rx-331": [16012],
    "tentec-rx-340": [16004],
    "tentec-rx-350": [16005],
    "tentec-delta-ii": [3064],
    "tentec-omni-vi-plus": [3051],
    "elecraft-xg3": [2038],
    "sigfox-transfox": [2032],
    "tentec-tt-585": [16009],
    "micom-2-3": [38001],
    "n2adr-hiqsdr": [25014],
    "mds-4710": [36001],
    "mds-9710": [36002],
    "tentec-tt-516": [16007],
    "tentec-tt-550": [16001],
    "tentec-tt-538": [16002],
    "tentec-tt-565": [16008],
    "tentec-tt-588": [16011],
    "tentec-tt-599": [16013],
    "yaesu-ft-736r": [1010],
    "yaesu-ft-817": [1020],
    "yaesu-ft-847": [1001],
    "yaesu-ft-847uni": [1038],
    "yaesu-ft-650": [1047],
    "yaesu-ft-897": [1023],
    "softrock-si570-avr-usb": [25009],
    "kth-si570-pic-usb": [25011],
    "ae9rb-peaberry-v1": [25016],
    "ae9rb-peaberry-v2": [25017],
    "funkamateur-fa-sdr": [25015],
    "fifi-sdr": [25012],
    "amsat-uk-funcube-dongle": [25013],
    "amsat-uk-funcube-dongle-pro-plus": [25018],
    "elektor-sdr-usb": [25007],
    "digital-world-traveller": [25003],
    "yaesu-ft-818": [1041],
    "yaesu-ft-857d": [1022],
    "yaesu-ft-897d": [1043],
    "yaesu-ft-747gx": [1005],
    "yaesu-ft-757gx": [1006],
    "yaesu-ft-757gxii": [1007],
    "yaesu-ft-767gx": [1009],
    "yaesu-ft-840": [1011],
    "yaesu-ft-890": [1015],
    "yaesu-ft-900": [1013],
    "yaesu-ft-920": [1014],
    "yaesu-ft-990": [1016],
    "yaesu-ft-990-oldrom": [1048],
    "yaesu-ft-1000": [1002],
    "yaesu-ft-1000d": [1003],
    "yaesu-ft-1000mp": [1024],
    "yaesu-ft-1000mp-mkv": [1004],
    "yaesu-ft-1000mp-mkv-field": [1025],
    "yaesu-ft-100": [1021],
    "yaesu-ft-600": [1039],
    "yaesu-ft-980": [1031],
    "yaesu-frg-100": [1017],
    "yaesu-frg-8800": [1019],
    "yaesu-frg-9600": [1018],
    "yaesu-vr-5000": [1026],
    "vertex-vx-1700": [1033],
    "yaesu-ft-991a": [1035],
    "yaesu-ftdx10": [1042],
    "yaesu-ftdx101d": [1040],
}

# Hamlib model id -> why radioctl cannot support the model (status `not applicable`). Only for models with
# no documented serial, USB or network protocol that a program may speak itself; a reason is mandatory.
_WINRADIO_DRIVER = (
    "WiNRADiO: Hamlib reaches the radio only through the vendor's driver API (a Linux kernel driver's ioctl "
    "interface or the Windows vendor DLL); the radio's own USB protocol is not published, so radioctl has no wire "
    "protocol to speak"
)
_NOT_A_RADIO = "not a radio: a simulator; radioctl has its own virtual rigs (the virtual transport and the per-family twins)"
_RIG_SERVER_CLIENT = "not a radio: a client of another rig server or logging program; radioctl talks to the radio itself"
_SOFTWARE_RADIO = (
    "not a radio: a software SDR or DSP program that offers a control interface of its own; radioctl drives the "
    "radio, not the program in front of it"
)
_SOFTWARE_CAT = (
    "not a radio: a software SDR program that imitates a Kenwood CAT server; radioctl drives the radio itself "
    "(the Kenwood TS-2000 and TS-480 descriptions fit a program that imitates them)"
)
_KERNEL_TUNER = (
    "Video4Linux: Hamlib reaches the tuner only through the kernel's V4L ioctl interface (a driver, not a wire "
    "protocol), and it is an FM/AM broadcast tuner card; radioctl has no device-driver transport"
)
_MEASURING = (
    "not a radio: a vector network analyser (antenna and component measurement) that Hamlib lists as a tuner so "
    "programs can sweep it; radioctl drives radios"
)
_PARALLEL_PORT = (
    "a parallel-port device: Hamlib bit-bangs the printer port, and radioctl has no parallel-port transport "
    "(serial, TCP, UDP only)"
)
_BIT_BANGED_SERIAL = (
    "a DRM receiver kit whose DDS chip Hamlib steers by bit-banging the serial port's TX, RTS and DTR lines: "
    "there is no byte protocol on the wire, and radioctl's serial transport sends bytes, not line toggles"
)
NOT_APPLICABLE: dict[int, str] = {
    1: _NOT_A_RADIO,
    6: _NOT_A_RADIO,
    2: _RIG_SERVER_CLIENT,
    4: _RIG_SERVER_CLIENT,
    5: _RIG_SERVER_CLIENT,
    8: _RIG_SERVER_CLIENT,
    9: _RIG_SERVER_CLIENT,
    10: _SOFTWARE_RADIO,
    23003: _SOFTWARE_RADIO,
    23004: _SOFTWARE_RADIO,
    2040: _SOFTWARE_CAT,
    2048: _SOFTWARE_CAT,
    2051: _SOFTWARE_CAT,
    2054: _SOFTWARE_CAT,
    2056: _SOFTWARE_CAT,
    26001: _KERNEL_TUNER,
    26002: _KERNEL_TUNER,
    25008: _MEASURING,
    23001: _PARALLEL_PORT,
    25001: _BIT_BANGED_SERIAL,
    25006: _PARALLEL_PORT,
    15001: _WINRADIO_DRIVER,
    15002: _WINRADIO_DRIVER,
    15003: _WINRADIO_DRIVER,
    15004: _WINRADIO_DRIVER,
    15005: _WINRADIO_DRIVER,
    15006: _WINRADIO_DRIVER,
    15007: _WINRADIO_DRIVER,
    15009: _WINRADIO_DRIVER,
}

# Hamlib family hint (derived.family) -> (tier, radioctl engine name or None).
FAMILY_TIERS: dict[str, tuple[str, str | None]] = {
    "icom_civ": ("A", "civ"),
    "kenwood_ascii": ("A", "kenwood"),
    "yaesu_newcat_ascii": ("A", "yaesu"),
    "elecraft_ascii": ("A", "elecraft"),
    "flexradio_network": ("A", "flex"),
    "yaesu_legacy": ("B", "yaesu_legacy"),
    "kenwood_hh_ascii": ("B", "kenwood_hh"),
    "icom_pcr_ascii": ("C", None),
    "icom_marine": ("C", None),
    "yaesu_other": ("B", "yaesu_legacy"),
}

# Hamlib model id -> radioctl engine, for Tier C models whose engine exists (a Tier C model not listed here
# still `needs engine`). Descriptions of these models are `described`; an undescribed one is `engine only`.
MODEL_ENGINES: dict[int, str] = {
    **{i: "aor" for i in (5001, 5002, 5003, 5004, 5005, 5006, 5008, 5013, 5014, 5015, 5016)},
    **{i: "civ" for i in (3052, 3053)},
    **{i: "rs" for i in (27001, 27002, 27003, 27004, 27005)},
    **{i: "icom_marine" for i in (30001, 30002, 30003, 30004)},
    **{i: "icom_pcr" for i in (4001, 4002, 4003, 4004)},
    **{i: "jrc" for i in (6001, 6002, 6005, 6006, 6007)},
    **{i: "uniden" for i in (8001, 8002, 8003, 8006, 8010, 8011, 8012)},
    # Tier D USB boards driven by the USB transport (usb_link).
    **{i: "si570_usb" for i in (25009, 25011, 25012, 25015, 25016, 25017)},
    **{i: "funcube" for i in (25013, 25018)},
    25003: "dwt",
}

# Manufacturers whose models (outside the families above) need a new engine: Tier C. Every other
# model in the remaining families is an odd device (SDR board, network driver, parallel, dummy): Tier D.
TIER_C_MAKERS = {
    "AOR", "Alinco", "Barrett", "CODAN", "Drake", "Guohe", "Hilberling", "JRC", "Kachina",
    "Lab599", "Lowe", "Micom", "Optoelectronics", "Racal", "Radio Shack", "Rhode&Schwarz",
    "Rohde&Schwarz", "Skanti", "Ten-Tec", "Uniden", "Watkins-Johnson", "Winradio",
}

TIER_TITLES = {
    "A": "data only on an existing engine",
    "B": "variants of an existing engine",
    "C": "new engine",
    "D": "odd device",
}
STATUS_ORDER = ["described", "engine only", "needs variant", "needs engine", "odd device", "not applicable"]


# [@ANCHOR: radioctl_support_load_descriptions]
def load_descriptions(rigs_dir: Path = RIGS_DIR) -> dict[str, dict]:
    """Reads rigs/*.toml: id, name, family, highest verified state and hardware facts."""
    found = {}
    for path in sorted(rigs_dir.glob("*.toml")):
        data = tomllib.loads(path.read_text())
        states = [f["verified"]["state"] for f in data.get("facts", [])]
        top = max(states, key=LEVELS.index, default="unverified")
        hardware = [f["verified"] for f in data.get("facts", [])
                    if f["verified"]["state"] == "hardware-verified"]
        found[data["rig"]["id"]] = {
            "name": data["rig"]["name"],
            "family": data["rig"]["family"],
            "highest": top,
            "hardware": hardware,
        }
    return found


# [@ANCHOR: radioctl_support_load_reports]
def load_reports(path: Path = RIGS_DIR / "reports" / "verified.toml") -> list[dict]:
    """The [[report]] entries of rigs/reports/verified.toml."""
    return tomllib.loads(path.read_text()).get("report", [])


# [@ANCHOR: radioctl_support_check_consistency]
def check_consistency(descriptions: dict[str, dict], reports: list[dict], models: list[dict]) -> list[str]:
    """Problems that would make the table lie (empty when verified.toml, facts and map agree)."""
    problems = []
    ids = {m["rig_list"]["model_id"] for m in models}
    for rig_id in descriptions:
        if rig_id not in HAMLIB_IDS:
            problems.append(f"{rig_id}: add it to HAMLIB_IDS (use [] if Hamlib has no such model)")
    for rig_id, hamlib in HAMLIB_IDS.items():
        if rig_id not in descriptions:
            problems.append(f"HAMLIB_IDS names {rig_id}, which has no rigs/{rig_id}.toml")
        problems += [f"{rig_id}: Hamlib id {i} does not exist" for i in hamlib if i not in ids]
    reported = {r.get("rig") for r in reports}
    for r in reports:
        for key in ("rig", "radio", "firmware", "date", "reporter", "issue"):
            if not r.get(key):
                problems.append(f"verified.toml: report for {r.get('rig')!r} lacks {key}")
        if r.get("rig") not in descriptions:
            problems.append(f"verified.toml: {r.get('rig')!r} has no description")
        elif not descriptions[r["rig"]]["hardware"]:
            problems.append(f"verified.toml: {r['rig']} has no hardware-verified fact")
    for rig_id, d in descriptions.items():
        if d["hardware"] and rig_id not in reported:
            problems.append(f"{rig_id}: hardware-verified facts but no [[report]] in verified.toml")
    return problems


# [@ANCHOR: radioctl_support_classify]
def classify(model: dict) -> tuple[str, str, str | None]:
    """(family, tier, engine) for a Hamlib model, by this tool's own rules (see FAMILY_TIERS)."""
    family = (model.get("derived") or {}).get("family") or "unclassified"
    if family in FAMILY_TIERS:
        tier, engine = FAMILY_TIERS[family]
        if tier == "C":
            engine = MODEL_ENGINES.get(model["rig_list"]["model_id"], engine)
        return family, tier, engine
    if model["rig_list"]["mfg"] in TIER_C_MAKERS:
        return family, "C", MODEL_ENGINES.get(model["rig_list"]["model_id"])
    return family, "D", MODEL_ENGINES.get(model["rig_list"]["model_id"])


# [@ANCHOR: radioctl_support_status]
def status_of(tier: str, engine: str | None, described: bool, not_applicable: bool = False) -> str:
    """described / engine only / needs variant / needs engine / odd device / not applicable."""
    if described:
        return "described"
    if not_applicable:
        return "not applicable"
    if tier == "C" and engine:
        return "engine only"
    return {"A": "engine only", "B": "needs variant", "C": "needs engine", "D": "odd device"}[tier]


# [@ANCHOR: radioctl_support_build_rows]
def build_rows(models: list[dict], descriptions: dict[str, dict], reports: list[dict]) -> list[dict]:
    """One row per Hamlib model, sorted by manufacturer then model."""
    by_model: dict[int, list[str]] = {}
    for rig_id, hamlib in HAMLIB_IDS.items():
        for i in hamlib:
            by_model.setdefault(i, []).append(rig_id)
    by_rig = {r["rig"]: r for r in reports}
    rows = []
    for m in models:
        info = m["rig_list"]
        family, tier, engine = classify(m)
        rig_ids = sorted(by_model.get(info["model_id"], []))
        top = max((descriptions[r]["highest"] for r in rig_ids), key=LEVELS.index, default="unverified")
        who = []
        for r in rig_ids:
            if r in by_rig:
                who.append(f"{by_rig[r]['reporter']}, {by_rig[r]['date']}")
        rows.append({
            "id": info["model_id"], "mfg": info["mfg"], "model": info["model"], "family": family,
            "tier": tier, "engine": engine, "descriptions": rig_ids, "level": top,
            "who": "; ".join(who) or "-", "status": status_of(tier, engine, bool(rig_ids), info["model_id"] in NOT_APPLICABLE),
        })
    rows.sort(key=lambda r: (r["mfg"].lower(), r["model"].lower(), r["id"]))
    return rows


def cell(text: object) -> str:
    return str(text).replace("|", "\\|") or "-"


# [@ANCHOR: radioctl_support_render]
def render(rows: list[dict], descriptions: dict[str, dict]) -> str:
    """The Markdown file."""
    out = [
        "# Rig support matrix",
        "",
        "Generated by `tools/rig_support_matrix.py` from `data/hamlib-4.6.2/capability_matrix.json`, `rigs/*.toml` and "
        "`rigs/reports/verified.toml`. Do not edit by hand: `python3 tools/rig_support_matrix.py` rewrites it and "
        "`python3 tools/rig_support_matrix.py --check` (and the unit test) fail when it is stale.",
        "",
        "Policy (2026-10-31): radioctl implements every model Hamlib lists, keeps this list of which ones a volunteer has "
        "actually verified, and makes its best effort to work out of the box. Every level below `hardware-verified` "
        "means no one has confirmed the description on a radio. The plan is [RIG_COVERAGE_PLAN.md](RIG_COVERAGE_PLAN.md); "
        "to help test, see [VOLUNTEER_TESTING.md](VOLUNTEER_TESTING.md).",
        "",
        "Columns: **Tier** and **Family** are this tool's planning hints (not a statement of a radio's wire protocol); "
        "**Engine** is the radioctl protocol engine for the family, `-` when none exists; **Verification** is the highest "
        "`verified` state among the description's facts; **Verified by** comes from `rigs/reports/verified.toml`. "
        "Status: `described` = a `rigs/*.toml` exists; `engine only` = the engine exists, data missing; "
        "`needs variant` = an existing engine needs model variants; `needs engine` = a new engine; "
        "`odd device` = SDR board, network driver, parallel or dummy; `not applicable` = no wire protocol radioctl can "
        "speak (the reason is in the section below).",
        "",
    ]
    total = len(rows)
    described = sum(1 for r in rows if r["descriptions"])
    na = sum(1 for r in rows if r["status"] == "not applicable")
    hw = sum(1 for r in rows if r["level"] == "hardware-verified")
    out += [
        "## Counts",
        "",
        f"- Hamlib 4.6.2 models: **{total}**",
        f"- Not applicable (not a radio, or no transport radioctl has): **{na}**",
        f"- Real radios (models less not applicable): **{total - na}**; with a radioctl description: "
        f"**{described}** ({len(descriptions)} description files), **{described * 100 // (total - na)}%** of the real radios",
        f"- Hardware-verified: **{hw}**",
        "",
        "| Status | Models |",
        "|---|---:|",
    ]
    status_counts = Counter(r["status"] for r in rows)
    out += [f"| {s} | {status_counts.get(s, 0)} |" for s in STATUS_ORDER]
    out += ["", "| Verification level | Models |", "|---|---:|"]
    level_counts = Counter(r["level"] for r in rows)
    out += [f"| {lv} | {level_counts.get(lv, 0)} |" for lv in LEVELS]
    out += ["", "### By tier", "", "| Tier | Meaning | Models | Described |", "|---|---|---:|---:|"]
    for tier in "ABCD":
        sel = [r for r in rows if r["tier"] == tier]
        out.append(f"| {tier} | {TIER_TITLES[tier]} | {len(sel)} | {sum(1 for r in sel if r['descriptions'])} |")
    out += ["", "### By family", "", "| Family | Tier | Engine | Models | Described | Hardware-verified |",
            "|---|---|---|---:|---:|---:|"]
    families = sorted({r["family"] for r in rows}, key=lambda f: (-sum(1 for r in rows if r["family"] == f), f))
    for fam in families:
        sel = [r for r in rows if r["family"] == fam]
        tiers = "/".join(sorted({r["tier"] for r in sel}))
        engine = next((r["engine"] for r in sel if r["engine"]), None)
        out.append(
            f"| {fam} | {tiers} | {engine or '-'} | {len(sel)} | {sum(1 for r in sel if r['descriptions'])} | "
            f"{sum(1 for r in sel if r['level'] == 'hardware-verified')} |"
        )
    out += ["", "## Descriptions without a Hamlib model", ""]
    mapped = {rid for r in rows for rid in r["descriptions"]}
    orphans = sorted(set(descriptions) - mapped)
    out += [f"- `{rid}` ({descriptions[rid]['name']}): highest level {descriptions[rid]['highest']}" for rid in orphans] or ["- none"]
    out += ["", "## Models that are not applicable", ""]
    out += [f"- {r['mfg']} {r['model']} ({r['id']}): {NOT_APPLICABLE[r['id']]}" for r in rows if r["id"] in NOT_APPLICABLE] or ["- none"]
    out += ["", "## Every model", "",
            "| Hamlib id | Manufacturer | Model | Tier | Family | Engine | Description | Verification | Verified by | Status |",
            "|---:|---|---|---|---|---|---|---|---|---|"]
    for r in rows:
        desc = ", ".join(f"`{d}`" for d in r["descriptions"])
        out.append("| " + " | ".join(cell(x) for x in [
            r["id"], r["mfg"], r["model"], r["tier"], r["family"], r["engine"] or "-", desc or "-",
            r["level"], r["who"], r["status"]]) + " |")
    return "\n".join(out) + "\n"


# [@ANCHOR: radioctl_support_generate]
def generate() -> str:
    """The Markdown for the repository as it is now; raises ValueError listing any inconsistency."""
    models = json.loads((HAMLIB_DIR / "capability_matrix.json").read_text())["models"]
    descriptions = load_descriptions()
    reports = load_reports()
    problems = check_consistency(descriptions, reports, models)
    if problems:
        raise ValueError("\n".join(problems))
    return render(build_rows(models, descriptions, reports), descriptions)


# [@ANCHOR: radioctl_support_main]
def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--check", action="store_true", help="exit 1 if docs/RIG_SUPPORT.md is stale")
    args = parser.parse_args(argv)
    try:
        text = generate()
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1
    if args.check:
        if not OUT_PATH.exists() or OUT_PATH.read_text() != text:
            print("docs/RIG_SUPPORT.md is stale: run python3 tools/rig_support_matrix.py", file=sys.stderr)
            return 1
        return 0
    OUT_PATH.write_text(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
