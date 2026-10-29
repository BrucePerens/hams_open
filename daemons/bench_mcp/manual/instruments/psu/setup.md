---
title: Supply setup
summary: Serial port, configuration entry, caps and first checks.
see_also: [instruments/psu/overview, instruments/psu/troubleshooting, concepts/configuration]
instrument: psu
verified: code only, 2026-10-29
---

# Supply setup

```json
"psu": {"driver": "atm6301", "tier": "read_config", "serial": "/dev/ttyUSB0", "baud": 115200, "max_volts": 5.5, "max_amps": 1.0}
```

`serial` is required; `baud` defaults to 115200; `max_volts` and `max_amps` must be positive (defaults 5.5 and 1.0). Set the caps to the most the bench's loads may ever see, not to the supply's capability. The serial port is locked with `flock`, so two server processes cannot talk to it at once; a second one waits.

First check: `bench_status {"instrument": "psu"}` shows `output` off, the setpoints, the supply's own `ovp_v` and `ocp_a`, and the last entry of its error queue. Raise the tier to `full` in the configuration only if the person wants the output switched on by the agent.
