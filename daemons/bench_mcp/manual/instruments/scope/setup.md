---
title: Scope setup
summary: Network address, configuration entry, probes and the first checks.
see_also: [instruments/scope/overview, instruments/scope/troubleshooting, concepts/configuration]
instrument: scope
verified: code only, 2026-10-29
---

# Scope setup

Configuration entry (see [configuration](concepts/configuration)):

```json
"scope": {"driver": "siglent_sds", "tier": "read_config", "host": "192.168.10.51", "port": 5025}
```

`host` is required, `port` defaults to 5025, `tier` defaults to `read_only`. The scope needs its LAN address set and SCPI over sockets enabled in its utility menu; there is no way to do that through the tools.

First checks: `bench_identify` (the `*IDN?` string), then `bench_status`. Probes: set `attenuation` with [bench_set](tools/bench_set) to the probe's ratio (1 or 10) or all voltages are off by that factor. At 1 MOhm a 50 ohm source is read at twice its loaded level; terminate it in 50 ohms (a tee and a 50 ohm load) if you need `dbm_50` to be right.

Display: the screen blanks after 30 minutes (`SCSV 30MIN`). Measurements, captures and `SCDP` screenshots still work while it sleeps.
