---
title: Glasgow limits
summary: Ranges and where each comes from, what is not verified, and the legal and safety notes.
see_also: [instruments/glasgow/overview, instruments/glasgow/troubleshooting]
instrument: glasgow
verified: measured on the unit (Glasgow revC3, device firmware API level 3, glasgow software 70cb38c9), 2026-11-08
---

# Glasgow limits

| Limit | Value | Source |
|---|---|---|
| Port voltage | 1.65 to 5.0 V (revA to C), 0.9 to 5.5 V (revD); 0 = off | `glasgow voltage --help` of the pinned install; the config's `max_volts` can only lower it |
| Voltage limit register | set equal to the supply by `bench_glasgow_port_set`; the device reports 5.5 V when idle | `glasgow voltage-limit`, measured idle |
| Supply current trip | 0 to 0.325 A, default 0.1 A, config `max_trip_a` | the device (revC2 and later) |
| Time a port stays powered | `max_on_s`, default 600 s | the config; a watchdog thread switches ports off |
| Capture length | `max_capture_s`, default 30 s, at most 120 s; file at most `max_capture_mb` (default 64, at most 512) | the config |
| Applet call | `op_timeout_s`, default 120 s; hard stop by SIGINT then SIGTERM; never SIGKILL | the config |
| Pins | A0 to A7, B0 to B7; 16 for the analyzer, 8 for a pattern | the hardware and the tools |
| Logic analyzer | 48 MHz sample clock, no trigger (revC analyzer applet) | the applet |
| Pattern timing | about 1 ms, from the host | the tool |

Measured 2026-11-08 on the unit: ports idle at 0.000 V with limit 5.5 V, current trip 0.100 A, no fault. A gateware build of the benchmark, control-gpio and uart applets: 4.5 to 15 s each. The register-transfer applet `control-gpio` ran at full speed; the bulk applets did not (see [troubleshooting](instruments/glasgow/troubleshooting)).

Not verified on hardware: setting a port voltage, every pin tool, every capture. Their tests use a simulated device and the real helper script (`verified: fixtures only`). The first wired session is [bench-session](instruments/glasgow/bench-session).

Legal and safety: the Glasgow's pins can drive a target. A person who has checked the target's I/O voltage and wiring decides; compliance and any damage are the owner's responsibility. Nothing here radiates.
