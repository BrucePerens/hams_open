---
title: ATM6301 supply overview
summary: What the bench power supply is and what the tools can do with it.
see_also: [instruments/psu/setup, instruments/psu/operation, instruments/psu/limits]
tools: [bench_status, bench_measure, bench_set, bench_psu_output]
instrument: psu
verified: code only, 2026-10-29
---

# ATM6301 supply overview

An Abestop ATM6301 programmable bench supply, 0 to 60 V and 0 to 10 A, driver `atm6301`, on a CH340 USB-serial adapter (typically `/dev/ttyUSB0`). SCPI at 115200 baud, 8N1 (no answer at other rates). Tools: state ([bench_status](tools/bench_status)), measured volts, amps and watts ([bench_measure](tools/bench_measure)), setpoints ([bench_set](tools/bench_set)) and the output switch ([bench_psu_output](tools/bench_psu_output)).

The server adds protections the supply does not have: setpoints are capped by `max_volts` and `max_amps` (defaults 5.5 V and 1.0 A), and the output is switched on only at tier `full` and only if the setpoints already stored in the supply are within the caps (a front-panel value may have been left there; one unit came up at 12 V and 7.8 A).
