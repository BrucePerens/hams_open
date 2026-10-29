---
title: Unit monitoring, failures and recovery
summary: What the VNA reports about itself (firmware, temperature, overruns, recovered failures) and what the driver does when a measurement fails.
see_also: [instruments/vna/troubleshooting, instruments/vna/limits]
tools: [bench_status, bench_vna_sweep, bench_vna_power_cycle]
instrument: vna
tier: read_only
verified: measured on the unit (VNWA 3E, firmware v5.22), 2026-10-29
---

# Unit monitoring, failures and recovery

**Temperature.** `bench_status` reports `instrument_monitoring` with the firmware (v5.22 on the unit under test) and `temperature_counts`: the unit's internal sensor in raw counts.
The scale in degrees is not known; only changes mean anything. After a power-up the reading rises by about 100 counts in the first 5 minutes and then holds within a few counts. A loopback
sweep changes by less than 0.04 % in magnitude and 0.01 degrees in phase over that warm-up. `bench_vna_sweep` returns `temperature_counts_after`.

**Audio overruns.** The two audio streams are watched for buffer overruns. A measurement window that contains one is never used: the point is measured again, from a separate budget of 10 attempts
that never power-cycles the unit. `bench_vna_sweep` returns `recovery.audio_overruns_reported` and `recovery.points_measured_again_for_audio_overruns`. On the dev box normal operation reports none;
a power cycle with the audio streams left open reports thousands a second, which is why the driver closes them first.

**Failures that were cured.** A point that needed more than one attempt is measured, but its failed attempts are kept: `recovered_failures` in the sweep result lists the last five with the error text, the raw
channel levels, how long the set-up block took to send, and the last 16 USB transfers with their outcome. About 0.6 % of attempts still fail at the first try (a transfer error on the unit's read
of request 0x0d); before the window was timed from the end of the block send, 3 % failed.

**Power cycles.** After 3 failed attempts of one point the unit is cold reset once through its hub and the point tried 3 more times, then the call fails with every attempt listed. `bench_vna_power_cycle` does the
same on request. Everything the unit holds is lost.
