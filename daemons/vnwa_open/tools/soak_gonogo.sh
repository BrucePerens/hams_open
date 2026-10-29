#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# soak_gonogo.sh [run_dir] : a 3-minute go/no-go before the long soak: the soak itself for 0.05 hours (about 5 rounds: 60-point sweep against its baseline, the frequency meter on the
# loopback, the generator at -6 and -12 dB and off on the scope), then a verdict from its log. GO needs: no WRONG number and no error event, at least 4 rounds, the generator readings
# near 5.17, 2.59 and 0.14 mV (within 8 %), the sweep's worst deviation under 1 % and 1 degree (the soak's own tolerance), the meter under 0.01 Hz. Needs the unit on the bus, the scope on the tee and
# nothing else using either. Same binary location as soak_run.sh.
set -u
dir=$(realpath -m "${1:-$HOME/.claude-tmp/x/gonogo_$(date +%s)}")
target=${CARGO_TARGET_DIR:-$HOME/.cache/hams-cargo-target-vna-soak}
here=$(dirname "$(realpath "$0")")
mkdir -p "$dir"
"$here/soak_run.sh" 0.05 "$dir" 30 > /dev/null 2>&1
python3 - "$dir" <<'PY'
import json, re, sys
d = sys.argv[1]
log = open(d + "/soak.log").read().splitlines()
gen = [l for l in log if " GENERATOR " in l]
sw = [l for l in log if " SWEEP " in l]
meter = [l for l in log if " METER " in l]
events = []
try:
    events = [json.loads(l) for l in open(d + "/events.jsonl")]
except FileNotFoundError:
    pass
bad_events = [e for e in events if e["class"] in ("wrong_number", "unrecovered_failure", "generator_step_failed", "hang", "silent_state", "unscheduled_power_cycle", "unexpected_disconnect")]
problems = []
if len(sw) < 4:
    problems.append("only %d complete rounds" % len(sw))
for l in gen:
    m = re.search(r"-6 dB ([\d.]+) mV, -12 dB ([\d.]+) mV, off ([\d.]+) mV", l)
    if not m:
        continue
    a, b, c = map(float, m.groups())
    if abs(a / 5.17 - 1) > 0.08 or abs(b / 2.59 - 1) > 0.08 or c > 0.4:
        problems.append("generator read %s" % l.split("GENERATOR ")[1])
for l in sw:
    m = re.search(r"vs last-cycle baseline \(at \d+\) ([\d.]+) % ([\d.]+) deg", l)
    if m and (float(m.group(1)) > 1.0 or float(m.group(2)) > 1.0):
        problems.append("sweep deviates " + m.group(0))
for l in meter:
    m = re.search(r"offset ([+-][\d.]+) Hz", l)
    if m and abs(float(m.group(1))) > 0.01:
        problems.append("meter " + l.split("METER ")[1])
for e in bad_events:
    problems.append("event %s: %s" % (e["class"], e["summary"][:120]))
print("rounds %d, generator lines %d, events %d (soft events are not failures: %s)" % (len(sw), len(gen), len(events), sorted({e["class"] for e in events if e not in bad_events})))
if gen:
    print("last generator line:", gen[-1].split("GENERATOR ")[1])
print("NO-GO: " + "; ".join(problems) if problems else "GO")
sys.exit(1 if problems else 0)
PY
