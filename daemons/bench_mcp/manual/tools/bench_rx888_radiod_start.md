---
title: bench_rx888_radiod_start
summary: RX-888: starts radiod for the booted RX-888 on loopback (ttl 0), niced, and waits for its front end.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_radiod_start]
instrument: rx888
tier: read_config
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_radiod_start

Writes `rx888-<instrument>.conf` in `BENCH_MCP_OUT` (`iface = lo`, `ttl = 0`, `advertise = no`, the driver by `library =`). Starts `nice -n 10 radiod -v` on it with the log in `rx888-<instrument>.log` and the pid in `rx888-<instrument>.pid`, then waits up to 15 s for the front end to answer. Refused when the RX-888 is not booted, when loopback multicast is off, or when a `radiod` started by this or an earlier server still runs.

## Arguments

<!-- arguments -->

## Returns

`started`, `pid`, `config`, `log`, `front_end`.

## Failure modes

* `multicast is off on the loopback interface`: a person runs `ip link set dev lo multicast on` as root.
* `radiod exited at start` or `did not answer`: the error carries the log tail; see [troubleshooting](instruments/rx888/troubleshooting).

Example: `bench_rx888_radiod_start {}` returned `{"started": true, "front_end": {"input_sample_rate_hz": 64800000, "rf_agc": true, ...}}`.
