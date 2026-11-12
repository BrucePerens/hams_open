#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Totals of the RPC guard's report-mode log lines, read from Odoo's log on stdin or from files.

    sudo journalctl -u odoo --since "-1 day" | python3 report_summary.py
    python3 report_summary.py /var/log/odoo/odoo-server.log

The guard counts per worker process and writes a line at the 1st, 10th, 100th, ... would-be refusal of each kind
(`policy.record_verdict`, `endpoint_gate.note_request`), never an address, login, record id or argument. This reads the
two kinds of line (`odoo.addons.hams_rpc_guard.policy` for a model method, `odoo.addons.hams_rpc_guard.endpoint_gate` for
an endpoint) and prints, per kind, the sum over the workers of the highest count each logged: a lower bound, because a worker
that counted 57 has logged only the 10. Workers that were restarted count again from 1 (a new process id), so the sum
covers the whole span of the input. Lines of other loggers and other messages are ignored.

Output columns: lower-bound count, `model.method` or endpoint family, who (user class or client class), reason.
"""
import argparse
import collections
import re
import sys

POLICY = re.compile(
    r"^(?P<when>\S+ \S+) (?P<pid>\d+) \w+ \S+ odoo\.addons\.hams_rpc_guard\.policy: RPC guard \((?P<mode>\w+)\): "
    r"(?P<count>\d+) call\(s\) to (?P<what>\S+) by an? (?P<who>\w+) user, reason (?P<reason>[\w-]+)\.")
GATE = re.compile(
    r"^(?P<when>\S+ \S+) (?P<pid>\d+) \w+ \S+ odoo\.addons\.hams_rpc_guard\.endpoint_gate: "
    r"(?P<verb>Served \(report-only mode\), would have refused,|Refused) (?P<count>\d+) request\(s\) to the (?P<what>\S+) "
    r"endpoints from an? (?P<who>\S+) client \(reason: (?P<reason>[^)]+)\)")


# [@ANCHOR: hams_rpc_guard:report_summary]
def summarise(lines):
    """`{(kind, mode, what, who, reason): lower-bound total}` for the guard's would-be-refusal and refusal lines in `lines`.

    `kind` is `method` or `endpoint`; `mode` is the guard's mode (`report` or `enforce`; `endpoint` lines are `report` when
    served and `enforce` when refused). The total adds, over the process ids, the highest count each logged for the key."""
    latest = collections.defaultdict(dict)
    for line in lines:
        match = POLICY.match(line)
        if match:
            key = ("method", match["mode"], match["what"], match["who"], match["reason"])
        else:
            match = GATE.match(line)
            if not match:
                continue
            mode = "report" if match["verb"].startswith("Served") else "enforce"
            key = ("endpoint", mode, match["what"], match["who"], match["reason"])
        pid = match["pid"]
        latest[key][pid] = max(latest[key].get(pid, 0), int(match["count"]))
    return {key: sum(by_pid.values()) for key, by_pid in latest.items()}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("files", nargs="*", help="log files (default: standard input)")
    args = parser.parse_args(argv)
    lines = []
    if args.files:
        for name in args.files:
            with open(name, encoding="utf-8", errors="replace") as handle:
                lines.extend(handle)
    else:
        lines = sys.stdin
    totals = summarise(lines)
    if not totals:
        sys.stdout.write("No RPC guard would-be refusals or refusals in the input.\n")
        return 0
    for (kind, mode, what, who, reason), count in sorted(totals.items(), key=lambda item: (-item[1], item[0])):
        sys.stdout.write("%8d  %-8s %-7s %-60s %-10s %s\n" % (count, kind, mode, what, who, reason))
    return 0


if __name__ == "__main__":
    sys.exit(main())
