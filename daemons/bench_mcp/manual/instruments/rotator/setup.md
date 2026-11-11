---
title: Rotator setup
summary: Starting radioctld with a rotator section and the config entry with every key.
see_also: [instruments/rotator/overview, instruments/rotator/limits, concepts/configuration]
instrument: rotator
verified: virtual rotator only, 2026-11-11
---

# Rotator setup

1. Find the rotator's model id in radioctl's `data/station/rotators.toml` (for example `yaesu-g-5500-gs232b`; `generic-rotctld` is for any rotator behind another rotator daemon).
2. Start `radioctld` with a rotator section for it, on this machine only: `[[rotator]]` with `name`, `model`, `listen = "127.0.0.1:4533"` and a `[rotator.transport]` (`kind = "serial"` with a stable `/dev/serial/by-id` path and `baud`; see [bench_usb_find](tools/bench_usb_find), or `kind = "virtual"` for a rotator that does not exist). The format is in chapter 9 of the radioctl manual. Starting it opens the serial port; do it with the rotator in a state where that is safe.
3. Add an entry to the bench config:

```json
{"instruments": {
  "rotor": {"driver": "rotator", "tier": "read_config", "radioctld_port": 4533,
            "min_az": 0, "max_az": 360, "min_el": 0, "max_el": 90,
            "tolerance_deg": 2, "max_wait_s": 120}
}}
```

| Key | Meaning |
|---|---|
| `radioctld_port`, `radioctld_host` | where radioctld's rotator listener listens; the host must be this machine unless `allow_remote_radioctld` is true (radioctld has no authentication). The old names `rotctld_port`, `rotctld_host` and `allow_remote_rotctld` were removed and give an error that names the new ones |
| `min_az`, `max_az`, `min_el`, `max_el` | the owner's limits in degrees; each narrows what the rotator reports. If neither the rotator nor the config gives limits, nothing is moved |
| `tolerance_deg` | how close the reported position must be to the target to count as arrived (0.1 to 20, default 2) |
| `max_wait_s` | the longest a call waits for a move (1 to 600, default 120) |
| `reply_timeout_s` | how long one command may take (1 to 30, default 3) |
| `tier` | `read_only` lists the status tool; `read_config` adds point, stop and park |
