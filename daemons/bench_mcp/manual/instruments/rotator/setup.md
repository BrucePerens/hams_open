---
title: Rotator setup
summary: Starting rotctld for the rotator and the config entry with every key.
see_also: [instruments/rotator/overview, instruments/rotator/limits, concepts/configuration]
instrument: rotator
verified: dummy rotator only, 2026-10-29
---

# Rotator setup

1. Install the Hamlib utilities (Debian: `apt install libhamlib-utils`) and find the model: `rotctl -l | grep -i <name>`.
2. Start `rotctld` for the rotator, on this machine only, for example `rotctld -m 202 -r /dev/serial/by-id/usb-EXAMPLE_rotator-if00-port0 -s 9600 -t 4533 -T 127.0.0.1`. Use a stable `/dev/serial/by-id` path (see [bench_usb_find](tools/bench_usb_find)). Starting it opens the serial port; do it with the rotator in a state where that is safe.
3. Add an entry to the bench config:

```json
{"instruments": {
  "rotor": {"driver": "rotator", "tier": "read_config", "rotctld_port": 4533,
            "min_az": 0, "max_az": 360, "min_el": 0, "max_el": 90,
            "tolerance_deg": 2, "max_wait_s": 120}
}}
```

| Key | Meaning |
|---|---|
| `rotctld_port`, `rotctld_host` | where `rotctld` listens; the host must be this machine unless `allow_remote_rotctld` is true (rotctld has no authentication) |
| `min_az`, `max_az`, `min_el`, `max_el` | the owner's limits in degrees; each narrows what the rotator reports. If neither the rotator nor the config gives limits, nothing is moved |
| `tolerance_deg` | how close the reported position must be to the target to count as arrived (0.1 to 20, default 2) |
| `max_wait_s` | the longest a call waits for a move (1 to 600, default 120) |
| `reply_timeout_s` | how long one command may take (1 to 30, default 3) |
| `tier` | `read_only` lists the status tool; `read_config` adds point, stop and park |
