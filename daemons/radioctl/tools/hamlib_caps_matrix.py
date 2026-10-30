#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Builds radioctl's Tier 1 capability matrix from `rigctl` output.

Provenance: the only input is the text printed by `rigctl -l` and
`rigctl -m N --dump-caps` of the installed Hamlib. That is program
output. This tool reads no Hamlib source and copies no Hamlib table.
Two kinds of field are kept apart in the output:

* `dump` fields are parsed from the program output;
* `derived` fields (protocol family, VFO model, quirk hints) are this
  tool's own classification rules, written here from vendor
  documentation and general knowledge, not taken from Hamlib. They are
  hints for planning, never a statement about a radio's wire protocol.

Usage:
    hamlib_caps_matrix.py --out-dir DIR [--jobs N]
    hamlib_caps_matrix.py --check --out-dir DIR

DIR receives capability_matrix.json and HAMLIB_DIFFERENCES.md.
`--check` regenerates in memory and exits 1 if the files differ.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

SCHEMA_VERSION = 1
RIGCTL = "rigctl"
FUNC_RE = re.compile(r"(\w+)\(([^)]*)\)")
RANGE_RE = re.compile(r"^\t(\d+) Hz - (\d+) Hz")
POWER_RE = re.compile(r"High power: (\d+) W")
SERIAL_RE = re.compile(
    r"Serial speed: (\d+)\.\.(\d+) baud, (\d)([NEO])(\d), ctrl=(\S+)"
)
TIMING_RE = re.compile(
    r"Write delay: (\d+)ms, timeout (\d+)ms, (\d+) retry"
)
RIT_RE = re.compile(r"Max (RIT|XIT|IF-SHIFT): (\S+)/(\S+)")
RANGE_HEAD_RE = re.compile(r"^(TX|RX) ranges #(\d+) for (\S+):$")
SCOPE_LEVEL_PREFIX = "SPECTRUM"
LONG_TIMEOUT_MS = 1000
HIGH_RETRY = 5
MAX_LISTED_MODELS = 40

NEWCAT_NAMES = {
    "FT-450",
    "FT-450D",
    "FT-950",
    "FT-2000",
    "FT-991",
    "FT-891",
    "FT-710",
}


# [@ANCHOR: radioctl_caps_parse_rig_list]
def parse_rig_list(text: str) -> list[dict]:
    """Parses `rigctl -l`; columns are found from the header."""
    lines = text.splitlines()
    head = lines[0]
    cols = [
        head.index(name)
        for name in ("Rig #", "Mfg", "Model", "Version", "Status")
    ]
    cols.append(head.index("Macro"))
    rows = []
    for line in lines[1:]:
        if not line.strip():
            continue
        cells = [
            line[start:end].strip()
            for start, end in zip(cols, cols[1:] + [len(line)])
        ]
        rows.append(
            {
                "model_id": int(cells[0]),
                "mfg": cells[1],
                "model": cells[2],
                "backend_version": cells[3],
                "status": cells[4],
                "macro": cells[5],
            }
        )
    return rows


def _words(value: str) -> list[str]:
    """Words of a list line; a `None...` note means an empty list."""
    if value.startswith("None"):
        return []
    return value.split()


def _entries(value: str) -> list[dict]:
    """`NAME(min..max/step)` lists, as printed for levels."""
    out = []
    for name, spec in FUNC_RE.findall(value):
        entry = {"name": name}
        match = re.match(
            r"^(-?[\d.]+)\.\.(-?[\d.]+)/(-?[\d.]+)$", spec
        )
        if match:
            entry["min"] = float(match.group(1))
            entry["max"] = float(match.group(2))
            entry["step"] = float(match.group(3))
        else:
            entry["values"] = spec
        out.append(entry)
    return out


def _tabbed(line: str) -> tuple[str, str]:
    key, _, value = line.partition(":")
    return key.strip(), value.strip()


# [@ANCHOR: radioctl_caps_parse_ranges]
def parse_ranges(lines: list[str]) -> dict:
    """Summarises the TX and RX frequency range blocks."""
    kinds: dict[str, dict] = {
        "TX": {"count": 0, "min_hz": None, "max_hz": None},
        "RX": {"count": 0, "min_hz": None, "max_hz": None},
    }
    kinds["TX"]["max_power_w"] = 0
    current = None
    for line in lines:
        head = RANGE_HEAD_RE.match(line.strip())
        if head:
            current = head.group(1)
            continue
        if line.strip().startswith(("TX ranges", "RX ranges")):
            current = None
            continue
        found = RANGE_RE.match(line)
        if found and current:
            low, high = int(found.group(1)), int(found.group(2))
            slot = kinds[current]
            slot["count"] += 1
            if slot["min_hz"] is None or low < slot["min_hz"]:
                slot["min_hz"] = low
            if slot["max_hz"] is None or high > slot["max_hz"]:
                slot["max_hz"] = high
        power = POWER_RE.search(line)
        if power and current == "TX":
            watts = int(power.group(1))
            kinds["TX"]["max_power_w"] = max(
                kinds["TX"]["max_power_w"], watts
            )
    return kinds


def _parse_blocks(lines: list[str]) -> dict:
    """Counts of tuning steps, filters and memory groups."""
    out = {"tuning_steps": 0, "filters": 0, "memory_groups": []}
    section = None
    for line in lines:
        if not line.startswith("\t"):
            section = line.split(":")[0].strip()
            continue
        if section == "Tuning steps" and "Hz:" in line:
            out["tuning_steps"] += 1
        elif section == "Filters" and "Hz:" in line:
            out["filters"] += 1
        elif section == "Memories" and re.match(
            r"^\t\d+\.\.\d+:", line
        ):
            kind = line.split(":", 1)[1].strip()
            out["memory_groups"].append(kind)
    return out


# [@ANCHOR: radioctl_caps_parse_dump]
def parse_dump(text: str) -> dict:
    """Parses one `--dump-caps` report into the `dump` fields."""
    lines = text.splitlines()
    keyed: dict[str, str] = {}
    cans: dict[str, str] = {}
    for line in lines:
        if line.startswith("\t") or ":" not in line:
            continue
        key, value = _tabbed(line)
        if key.startswith(("Can ", "Has ")):
            cans[key] = value
        keyed.setdefault(key, value)
    out: dict = {
        "model_name": keyed.get("Model name", ""),
        "mfg_name": keyed.get("Mfg name", ""),
        "backend_version": keyed.get("Backend version", ""),
        "backend_status": keyed.get("Backend status", ""),
        "backend_copyright": keyed.get("Backend copyright", ""),
        "rig_type": keyed.get("Rig type", ""),
        "port_type": keyed.get("Port type", ""),
        "dcd_type": keyed.get("DCD type", ""),
        "async_data": keyed.get("Has async data support") == "Y",
        "has_targetable_vfo": keyed.get("Has targetable VFO") == "Y",
        "targetable_features": _words(
            keyed.get("Targetable features", "").replace("None", "")
        ),
        "vfo_list": _words(keyed.get("VFO list", "")),
        "vfo_ops": _words(keyed.get("VFO Ops", "")),
        "scan_ops": _words(keyed.get("Scan Ops", "")),
        "modes": _words(keyed.get("Mode list", "")),
        "get_functions": _words(keyed.get("Get functions", "")),
        "set_functions": _words(keyed.get("Set functions", "")),
        "get_levels": _entries(keyed.get("Get level", "")),
        "set_levels": _entries(keyed.get("Set level", "")),
        "get_parameters": _entries(keyed.get("Get parameters", "")),
        "set_parameters": _entries(keyed.get("Set parameters", "")),
        "preamp_db": keyed.get("Preamp", ""),
        "attenuator_db": keyed.get("Attenuator", ""),
        "agc_levels": keyed.get("AGC levels", ""),
        "ctcss_tones": _count_tail(keyed.get("CTCSS", ""), "tones"),
        "dcs_codes": _count_tail(keyed.get("DCS", ""), "codes"),
        "banks": int(keyed.get("Number of banks", "0") or 0),
        "spectrum_scopes": keyed.get("Spectrum scopes", "None"),
        "spectrum_modes": keyed.get("Spectrum modes", ""),
        "spectrum_spans": _words(keyed.get("Spectrum spans", "")),
        "backend_warnings": keyed.get("Overall backend warnings", ""),
        "can": cans,
    }
    out["extra_levels"] = _extra_levels(lines)
    out.update(_parse_blocks(lines))
    out["ranges"] = parse_ranges(lines)
    _parse_serial_and_timing(text, out)
    _parse_rit(text, out)
    return out


def _count_tail(value: str, noun: str) -> int:
    if value in ("", "None"):
        return 0
    match = re.search(r"(\d+) " + noun, value)
    return int(match.group(1)) if match else 0


def _extra_levels(lines: list[str]) -> list[str]:
    names = []
    inside = False
    for line in lines:
        if line.startswith("Extra levels:"):
            inside = True
            continue
        if inside and line.startswith("\t") and line.count("\t") == 1:
            names.append(line.strip())
        elif inside and not line.startswith("\t"):
            break
    return names


def _parse_serial_and_timing(text: str, out: dict) -> None:
    serial = SERIAL_RE.search(text)
    out["serial"] = None
    if serial:
        out["serial"] = {
            "min_baud": int(serial.group(1)),
            "max_baud": int(serial.group(2)),
            "data_bits": int(serial.group(3)),
            "parity": serial.group(4),
            "stop_bits": int(serial.group(5)),
            "handshake": serial.group(6),
        }
    timing = TIMING_RE.search(text)
    post = re.search(r"Post write delay: (\d+)ms", text)
    out["timing"] = None
    if timing and post:
        out["timing"] = {
            "write_delay_ms": int(timing.group(1)),
            "timeout_ms": int(timing.group(2)),
            "retries": int(timing.group(3)),
            "post_write_delay_ms": int(post.group(1)),
        }


def _parse_rit(text: str, out: dict) -> None:
    out["limits"] = {}
    for kind, low, high in RIT_RE.findall(text):
        out["limits"][kind.lower().replace("-", "_")] = [low, high]


# [@ANCHOR: radioctl_caps_classify_family]
def classify_family(mfg: str, model: str, port_type: str) -> dict:
    """Derived protocol family: a planning hint, not Hamlib output.

    First match wins. `basis` says which rule fired so a reader can
    see how weak the evidence is.
    """
    if mfg == "Hamlib":
        return _family("virtual", "dummy backend")
    if mfg in ("Icom", "Xiegu"):
        if model.startswith("IC-PCR"):
            return _family("icom_pcr_ascii", "name prefix IC-PCR")
        if model.startswith("IC-M"):
            return _family("icom_marine", "name prefix IC-M")
        return _family("icom_civ", "manufacturer Icom or Xiegu")
    if mfg == "Elecraft":
        return _family("elecraft_ascii", "manufacturer Elecraft")
    if mfg == "Kenwood":
        if model.startswith(("TH-", "TM-")):
            return _family(
                "kenwood_hh_ascii", "name prefix TH- or TM-"
            )
        return _family("kenwood_ascii", "manufacturer Kenwood")
    if mfg in ("Yaesu", "Vertex Standard"):
        return _yaesu_family(model)
    if mfg in ("FlexRadio", "FlexRadio/Apache") and (
        "Network" in port_type
    ):
        return _family("flexradio_network", "Flex with network port")
    if mfg == "OpenHPSDR" and "Network" in port_type:
        return _family("hpsdr_network", "OpenHPSDR with network port")
    return _family(_port_family(port_type), "port type only")


def _yaesu_family(model: str) -> dict:
    if model in NEWCAT_NAMES or model.startswith("FTDX-"):
        return _family("yaesu_newcat_ascii", "model name list")
    if model.startswith(("FT-", "FRG-", "VR-", "MARK-V")):
        return _family("yaesu_legacy", "older Yaesu name prefix")
    return _family("yaesu_other", "unclassified Yaesu name")


def _port_family(port_type: str) -> str:
    table = {
        "RS-232": "other_serial",
        "USB": "other_usb",
        "Network link": "other_network",
        "UDP Network link": "other_network",
        "Device driver": "other_device_driver",
        "Parallel": "other_parallel",
    }
    return table.get(port_type, "other_none")


def _family(name: str, basis: str) -> dict:
    return {"family": name, "basis": basis}


# [@ANCHOR: radioctl_caps_vfo_model]
def vfo_model(vfo_list: list[str]) -> str:
    """Derived VFO model from the VFO names the dump prints."""
    names = set(vfo_list)
    if {"Main", "Sub"} <= names:
        return "main_sub"
    if {"VFOA", "VFOB"} <= names:
        return "a_b"
    if names & {"VFOA", "VFO"}:
        return "single"
    return "none"


def has_scope(dump: dict) -> bool:
    """True when any spectrum scope data is advertised."""
    if dump["spectrum_scopes"] not in ("None", ""):
        return True
    names = [e["name"] for e in dump["get_levels"]]
    return any(n.startswith(SCOPE_LEVEL_PREFIX) for n in names)


# [@ANCHOR: radioctl_caps_quirk_hints]
def quirk_hints(dump: dict) -> list[str]:
    """Derived timing and behaviour hints, from dump fields only."""
    hints = []
    timing = dump["timing"] or {}
    if timing.get("write_delay_ms", 0) > 0:
        hints.append("inter_byte_write_delay")
    if timing.get("post_write_delay_ms", 0) > 0:
        hints.append("post_write_delay")
    if timing.get("timeout_ms", 0) >= LONG_TIMEOUT_MS:
        hints.append("long_timeout")
    if timing.get("retries", 0) >= HIGH_RETRY:
        hints.append("many_retries")
    serial = dump["serial"] or {}
    if serial.get("handshake") not in (None, "NONE"):
        hints.append("flow_control_" + serial["handshake"].lower())
    if serial and serial["stop_bits"] == 2:
        hints.append("two_stop_bits")
    if serial and serial["data_bits"] == 7:
        hints.append("seven_data_bits")
    if dump["async_data"]:
        hints.append("async_data_capable")
    can = dump["can"]
    if can.get("Can set Transceive") == "E":
        hints.append("transceive_flag_E")
    if can.get("Can get PTT") == "N":
        hints.append("no_ptt_readback")
    if can.get("Can get Frequency") == "N":
        hints.append("no_frequency_readback")
    if dump["backend_status"] in ("Alpha", "Beta"):
        hints.append("backend_" + dump["backend_status"].lower())
    warn = dump["backend_warnings"]
    if warn and not warn.startswith("0"):
        hints.append("backend_self_reported_gaps")
    return hints


# [@ANCHOR: radioctl_caps_build_entry]
def build_entry(row: dict, dump: dict | None, error: str) -> dict:
    """One matrix entry: list-row identity plus dump and derived."""
    entry: dict = {"rig_list": row, "dump_error": error or None}
    if dump is None:
        entry["dump"] = None
        entry["derived"] = None
        return entry
    entry["dump"] = dump
    family = classify_family(
        row["mfg"], row["model"], dump["port_type"]
    )
    entry["derived"] = {
        "family": family["family"],
        "family_basis": family["basis"],
        "vfo_model": vfo_model(dump["vfo_list"]),
        "has_scope": has_scope(dump),
        "quirk_hints": quirk_hints(dump),
    }
    return entry


def run_rigctl(args: list[str]) -> tuple[int, str]:
    done = subprocess.run(
        [RIGCTL, *args],
        capture_output=True,
        text=True,
        check=False,
        timeout=60,
    )
    return done.returncode, done.stdout


def dump_one(row: dict) -> dict:
    code, text = run_rigctl(
        ["-m", str(row["model_id"]), "--dump-caps"]
    )
    if code != 0 or "Model name" not in text:
        error = "dump-caps exit %d" % code
        return build_entry(row, None, error)
    return build_entry(row, parse_dump(text), "")


# [@ANCHOR: radioctl_caps_build_matrix]
def build_matrix(jobs: int) -> dict:
    """Runs rigctl for every listed model and returns the matrix."""
    _, version = run_rigctl(["--version"])
    _, listing = run_rigctl(["-l"])
    rows = parse_rig_list(listing)
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        entries = list(pool.map(dump_one, rows))
    return {
        "schema_version": SCHEMA_VERSION,
        "provenance": {
            "source": "program output of rigctl, no Hamlib source",
            "rigctl_version": version.strip(),
            "commands": ["rigctl -l", "rigctl -m N --dump-caps"],
            "dump_fields": "parsed from the report",
            "derived_fields": "radioctl classification rules",
        },
        "models": entries,
    }


def _stats(values: list[int]) -> str:
    if not values:
        return "-"
    ordered = sorted(values)
    mid = ordered[len(ordered) // 2]
    return "%d / %d / %d" % (ordered[0], mid, ordered[-1])


def _model_list(entries: list[dict]) -> str:
    names = [
        "%d %s" % (e["rig_list"]["model_id"], e["rig_list"]["model"])
        for e in entries
    ]
    if len(names) > MAX_LISTED_MODELS:
        return "%d models (list omitted; see the JSON)" % len(names)
    return "; ".join(names)


def _ok(matrix: dict) -> list[dict]:
    return [e for e in matrix["models"] if e["dump"]]


def _table(header: list[str], rows: list[list[str]]) -> list[str]:
    out = ["| " + " | ".join(header) + " |"]
    out.append("|" + "|".join("---" for _ in header) + "|")
    for row in rows:
        out.append("| " + " | ".join(str(c) for c in row) + " |")
    return out + [""]


def _family_section(ok: list[dict]) -> list[str]:
    groups: dict[str, list[dict]] = {}
    for entry in ok:
        groups.setdefault(entry["derived"]["family"], []).append(
            entry
        )
    rows = []
    for family in sorted(groups, key=lambda f: (-len(groups[f]), f)):
        members = groups[family]
        bauds = [e["dump"]["serial"] for e in members]
        bauds = [b for b in bauds if b]
        timeouts = [
            e["dump"]["timing"]["timeout_ms"]
            for e in members
            if e["dump"]["timing"]
        ]
        low = min((b["min_baud"] for b in bauds), default="-")
        high = max((b["max_baud"] for b in bauds), default="-")
        rows.append(
            [
                family,
                len(members),
                "%s..%s" % (low, high),
                _stats(timeouts),
                sum(1 for e in members if e["dump"]["async_data"]),
                sum(1 for e in members if e["derived"]["has_scope"]),
            ]
        )
    head = [
        "Family",
        "Models",
        "Baud span",
        "Timeout ms min/median/max",
        "Async data",
        "Scope",
    ]
    return _table(head, rows)


def _manufacturer_section(ok: list[dict]) -> list[str]:
    groups: dict[str, list[dict]] = {}
    for entry in ok:
        groups.setdefault(entry["rig_list"]["mfg"], []).append(entry)
    rows = []
    for mfg in sorted(groups, key=lambda m: (-len(groups[m]), m)):
        ports = Counter(e["dump"]["port_type"] for e in groups[mfg])
        kinds = ", ".join("%s %d" % (k, v) for k, v in ports.items())
        rows.append([mfg, len(groups[mfg]), kinds])
    return _table(["Manufacturer", "Models", "Port types"], rows)


def _counter_table(
    title: str, counter: Counter, top: int
) -> list[str]:
    rows = [[k, v] for k, v in counter.most_common(top)]
    return ["### " + title, ""] + _table(["Name", "Models"], rows)


def _timing_sections(ok: list[dict]) -> list[str]:
    serial = [e["dump"]["serial"] for e in ok if e["dump"]["serial"]]
    spans = Counter(
        "%d..%d" % (s["min_baud"], s["max_baud"]) for s in serial
    )
    framing = Counter(
        "%d%s%d ctrl=%s"
        % (
            s["data_bits"],
            s["parity"],
            s["stop_bits"],
            s["handshake"],
        )
        for s in serial
    )
    timing = [e["dump"]["timing"] for e in ok if e["dump"]["timing"]]
    out = ["## Baud and timing", ""]
    out += _counter_table("Baud span", spans, 12)
    out += _counter_table("Framing and handshake", framing, 12)
    stats = [
        ["timeout_ms", _stats([t["timeout_ms"] for t in timing])],
        ["retries", _stats([t["retries"] for t in timing])],
        [
            "write_delay_ms",
            _stats([t["write_delay_ms"] for t in timing]),
        ],
        [
            "post_write_delay_ms",
            _stats([t["post_write_delay_ms"] for t in timing]),
        ],
    ]
    out += ["### Timing (min / median / max)", ""]
    out += _table(["Field", "Value"], stats)
    return out


def _vfo_sections(ok: list[dict]) -> list[str]:
    models = Counter(e["derived"]["vfo_model"] for e in ok)
    lists = Counter(" ".join(e["dump"]["vfo_list"]) for e in ok)
    feats = Counter(
        " ".join(e["dump"]["targetable_features"]) or "None"
        for e in ok
    )
    out = ["## VFO model", ""]
    out += _counter_table("Derived VFO model", models, 10)
    out += _counter_table("VFO list as printed", lists, 12)
    out += _counter_table("Targetable features", feats, 8)
    return out


def _feature_sections(ok: list[dict]) -> list[str]:
    funcs: Counter = Counter()
    levels: Counter = Counter()
    for entry in ok:
        funcs.update(entry["dump"]["get_functions"])
        levels.update(e["name"] for e in entry["dump"]["get_levels"])
    out = ["## Levels, functions and scope", ""]
    out += _counter_table("Get functions (top 25)", funcs, 25)
    out += _counter_table("Get levels (top 30)", levels, 30)
    scope = [e for e in ok if e["derived"]["has_scope"]]
    out += [
        "### Scope or spectrum data advertised",
        "",
        "%d models: %s" % (len(scope), _model_list(scope)),
        "",
    ]
    return out


def _hint_section(ok: list[dict]) -> list[str]:
    groups: dict[str, list[dict]] = {}
    for entry in ok:
        for hint in entry["derived"]["quirk_hints"]:
            groups.setdefault(hint, []).append(entry)
    out = [
        "## Quirk hints (derived)",
        "",
        "Rules in `quirk_hints()`; each reads dump fields only.",
        "",
    ]
    rows = [
        [h, len(groups[h]), _model_list(groups[h])]
        for h in sorted(groups, key=lambda h: (-len(groups[h]), h))
    ]
    return out + _table(["Hint", "Models", "Which"], rows)


def _model_rows(ok: list[dict]) -> list[list]:
    rows = []
    for entry in ok:
        row, dump = entry["rig_list"], entry["dump"]
        serial = dump["serial"]
        timing = dump["timing"] or {}
        baud = "-"
        if serial:
            baud = "%d..%d" % (serial["min_baud"], serial["max_baud"])
        rows.append(
            [
                row["model_id"],
                row["mfg"],
                row["model"],
                entry["derived"]["family"],
                dump["backend_status"],
                dump["port_type"],
                baud,
                "%s/%s/%s"
                % (
                    timing.get("timeout_ms", "-"),
                    timing.get("retries", "-"),
                    timing.get("write_delay_ms", "-"),
                ),
                entry["derived"]["vfo_model"],
                "Y" if entry["derived"]["has_scope"] else "N",
                len(dump["get_functions"]),
                len(dump["get_levels"]),
            ]
        )
    return rows


# [@ANCHOR: radioctl_caps_render_markdown]
def render_markdown(matrix: dict) -> str:
    """The generated differences document."""
    ok = _ok(matrix)
    failed = [e for e in matrix["models"] if not e["dump"]]
    prov = matrix["provenance"]
    out = ["# Hamlib model differences (generated)", ""]
    out += [
        "Generated by `tools/hamlib_caps_matrix.py` from the program "
        "output of `%s`. No Hamlib source was read and no Hamlib "
        "table was copied. Do not edit; regenerate per release."
        % (prov["rigctl_version"]),
        "",
        "Fields marked derived (family, VFO model, scope, quirk "
        "hints) come from radioctl's own rules in the script, not "
        "from Hamlib. Family is a planning hint; `family_basis` in "
        "the JSON says which rule fired.",
        "",
        "## Summary",
        "",
        "- Models listed by `rigctl -l`: %d" % len(matrix["models"]),
        "- Dumped: %d" % len(ok),
        "- Not dumpable: %d" % len(failed),
        "",
    ]
    status = Counter(e["dump"]["backend_status"] for e in ok)
    ports = Counter(e["dump"]["port_type"] for e in ok)
    out += _counter_table("Backend status", status, 10)
    out += _counter_table("Port type", ports, 10)
    out += ["## Protocol families (derived)", ""]
    out += _family_section(ok)
    out += ["## Manufacturers", ""]
    out += _manufacturer_section(ok)
    out += _timing_sections(ok)
    out += _vfo_sections(ok)
    out += _feature_sections(ok)
    out += _hint_section(ok)
    out += ["## Not dumpable", ""]
    for entry in failed:
        row = entry["rig_list"]
        out.append(
            "- %d %s %s: %s"
            % (
                row["model_id"],
                row["mfg"],
                row["model"],
                entry["dump_error"],
            )
        )
    out += ["", "## Per-model table", ""]
    head = [
        "ID",
        "Mfg",
        "Model",
        "Family",
        "Status",
        "Port",
        "Baud",
        "Timeout ms/retries/wdelay",
        "VFO",
        "Scope",
        "Get funcs",
        "Get levels",
    ]
    out += _table(head, _model_rows(ok))
    return "\n".join(out)


def render_json(matrix: dict) -> str:
    return json.dumps(matrix, indent=1, sort_keys=True) + "\n"


# [@ANCHOR: radioctl_caps_main]
def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    matrix = build_matrix(args.jobs)
    files = {
        "capability_matrix.json": render_json(matrix),
        "HAMLIB_DIFFERENCES.md": render_markdown(matrix) + "\n",
    }
    for name, content in files.items():
        path = args.out_dir / name
        if args.check:
            current = path.read_text() if path.exists() else ""
            if current != content:
                print("DRIFT: %s differs from rigctl output" % path)
                return 1
            continue
        args.out_dir.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        print("wrote %s" % path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
