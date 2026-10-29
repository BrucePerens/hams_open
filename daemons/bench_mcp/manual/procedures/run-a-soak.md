---
title: Start and stop a soak run
summary: The VNA soak harness: what it does each round, how to start and stop it and how to read the result.
see_also: [instruments/vna/monitoring-and-recovery, tools/bench_vna_power_cycle]
instrument: vna
verified: code only, 2026-10-29
---

# Start and stop a soak run

`daemons/vnwa_open/examples/soak.rs` is a long reliability run, started by the person or the VNA work from the shell, not through the MCP, because it holds the VNA and scope for hours (do not use them meanwhile). Each round: a 60-point sweep from 100 kHz to 150 MHz judged against the first sweep and against a baseline taken after each power cycle (0.3 % and 0.3 degrees, plus an allowance per 100 counts of temperature change), a frequency-meter loopback reading (under 0.01 Hz), the generator at -6 and -12 dB and off read on the scope, and a cold reset through the hub every 30 minutes. A clear error is counted as ERROR, a wrong number as WRONG, a step over its deadline as HANG (exit 3, and the wrapper restarts it).

Start it from `daemons/vnwa_open` under the wrapper with the hub configured; stop it with an interrupt. Read the run directory afterwards: the event list shows each deviation. Report counts of ERROR, WRONG and HANG, not an impression.
