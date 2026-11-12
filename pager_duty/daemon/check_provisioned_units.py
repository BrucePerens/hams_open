#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""
Standalone check script for a `pager.check` of type "synthetic": warns when something we built is not in operation.

hams1 ran for weeks without 28 tracked unit and drop-in files, a stale triage wrapper and a timer that was never enabled,
and no check noticed, because every existing check looks at what is installed. This one starts from what the
hams_shared MANIFEST says provisioning installs and asks systemd about each unit, so a unit that is missing, a timer that is
not running, a long-running service that is not active, or a path unit that is not watching is reported. A running thing that
fails is already the job of the "Systemd Failed Services Tracker".

What counts as in operation:

    unit file not known to systemd                         not installed
    .timer / .path not active                              not running   (a timer MANIFEST marks opt_in that is not enabled is
                                                                           deliberately only linked, and is left alone)
    .service that is not Type=oneshot, has no .timer or
    .path of its own, and is not active                    not running   (unless its Condition*= was evaluated and is unmet on this host)

Templates (`name@.service`), entries for a host class this host does not have, and units listed in HAMS_UNITS_EXPECTED_OFF
(comma separated unit names) or in units_expected_off.txt beside this script (tracked, one unit per line, `#` starts a comment,
each entry says why it is off and when to remove it) are not checked. The MANIFEST is read from the hams_shared tree beside this script's repository
(or HAMS_INFRASTRUCTURE_DIR), so it is always the one this deploy shipped.

Exit 0 healthy, 1 failing: `SEVERITY:high` (pages) on stderr, then a short first line (the monitor keeps about 80 characters) and
the full list.
"""

import os
import subprocess
import sys

DEFAULT_INFRASTRUCTURE_DIR = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "hams_shared", "tools"
)
SYSTEMD_DIR = "/opt/hams/systemd/"
UNIT_SUFFIXES = (".service", ".timer", ".path")
SHOW_PROPERTIES = "LoadState,ActiveState,UnitFileState,Type,ConditionResult,ConditionTimestamp"
SHORT_LIST = 4
SYSTEMCTL_TIMEOUT_SECONDS = 10  # a hung systemd must fail the check, not run into the monitor's own timeout
EXPECTED_OFF_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "units_expected_off.txt")

# The MANIFEST module is a hard dependency: reached by a sys.path hop (the odoo.addons namespace is not importable in this
# script's standalone process), imported at the top, and a missing one stops the script with a traceback (non-zero exit, so
# the check fails loudly) instead of being papered over by a soft-import fallback.
_INFRASTRUCTURE_DIR = os.environ.get("HAMS_INFRASTRUCTURE_DIR") or DEFAULT_INFRASTRUCTURE_DIR
if _INFRASTRUCTURE_DIR not in sys.path:
    sys.path.insert(0, _INFRASTRUCTURE_DIR)
import infrastructure  # noqa: E402


# [@ANCHOR: pager_duty:provisioned_units]
def expected_units(infra, environment="prod"):
    """{unit name: {"opt_in": bool, "oneshot": bool}} for every unit file the MANIFEST installs for this host."""
    units = {}
    for spec in infra.MANIFEST["static_files"]:
        path = spec["path"]
        if (environment not in spec["environments"] or os.path.dirname(path) + "/" != SYSTEMD_DIR
                or not path.endswith(UNIT_SUFFIXES) or not infra._in_host_class(spec)):
            continue
        name = os.path.basename(path)
        if "@" in name:
            continue
        content = spec.get("content") or ""
        units[name] = {"opt_in": bool(spec.get("opt_in")), "oneshot": "Type=oneshot" in content}
    return units


# [@ANCHOR: pager_duty:provisioned_units_systemctl_show]
def systemctl_show(unit):
    """{property: value} for one unit; an unreadable answer is an empty dict."""
    result = subprocess.run(["systemctl", "show", unit, "-p", SHOW_PROPERTIES], capture_output=True, text=True, check=False,
                            timeout=SYSTEMCTL_TIMEOUT_SECONDS)
    return dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)


# [@ANCHOR: pager_duty:units_not_in_operation]
def units_not_in_operation(units, show=systemctl_show, expected_off=()):
    """[(unit, reason)] sorted by unit, for the units in `units` that are not in operation (see the module docstring)."""
    has_trigger = {name[: name.rindex(".")] for name in units if name.endswith((".timer", ".path"))}
    problems = []
    for name, info in sorted(units.items()):
        if name in expected_off:
            continue
        state = show(name)
        if state.get("LoadState", "not-found") != "loaded":
            problems.append((name, "not installed"))
            continue
        active = state.get("ActiveState") == "active"
        if name.endswith((".timer", ".path")):
            if not active and not (info["opt_in"] and state.get("UnitFileState") != "enabled"):
                problems.append((name, "not running"))
        elif name.endswith(".service"):
            long_running = not info["oneshot"] and name[: -len(".service")] not in has_trigger
            # ConditionResult is "no" for a unit systemd has never evaluated too (ConditionTimestamp empty): a unit that was
            # installed and never started is the case this check exists for, so only an evaluated "no" excuses it.
            condition_unmet = state.get("ConditionResult", "yes") == "no" and bool(state.get("ConditionTimestamp"))
            deliberately_off = info["opt_in"] and state.get("UnitFileState") != "enabled"  # as for a timer
            if long_running and not active and not condition_unmet and not deliberately_off:
                problems.append((name, "not running"))
    return problems


# [@ANCHOR: pager_duty:units_expected_off]
def units_expected_off(environ=None, path=EXPECTED_OFF_FILE):
    """Set of unit names not to check: HAMS_UNITS_EXPECTED_OFF (comma separated) plus the tracked file `path` (one unit per line,
    `#` comments and blank lines ignored; a missing file adds nothing)."""
    environ = os.environ if environ is None else environ
    names = {name.strip() for name in environ.get("HAMS_UNITS_EXPECTED_OFF", "").split(",") if name.strip()}
    if os.path.exists(path):
        with open(path, encoding="utf-8") as listing:
            for line in listing:
                line = line.split("#", 1)[0].strip()
                if line:
                    names.add(line)
    return names


# [@ANCHOR: pager_duty:provisioned_units_main]
def main(argv=None):
    expected_off = units_expected_off()
    try:
        units = expected_units(infrastructure)
    except (OSError, KeyError) as problem:
        print("SEVERITY:high", file=sys.stderr)
        print(f"provisioned units cannot be checked: the MANIFEST did not load ({problem})", file=sys.stderr)
        return 1
    try:
        problems = units_not_in_operation(units, expected_off=expected_off)
    except subprocess.TimeoutExpired:
        print("SEVERITY:high", file=sys.stderr)
        print("provisioned units cannot be checked: systemctl did not answer in time", file=sys.stderr)
        return 1
    if not problems:
        print(f"all {len(units)} provisioned units are in operation")
        return 0
    names = [f"{unit} ({reason})" for unit, reason in problems]
    shown = ", ".join(unit for unit, _ in problems[:SHORT_LIST]) + (f" +{len(problems) - SHORT_LIST}" if len(problems) > SHORT_LIST else "")
    print("SEVERITY:high", file=sys.stderr)
    print(f"{len(problems)} provisioned unit(s) not in operation: {shown}", file=sys.stderr)
    print("; ".join(names), file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
