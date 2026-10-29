---
title: Example instrument setup
summary: Config entry, cabling and safe starting levels. The model for a setup page.
see_also: [instruments/example/overview, concepts/configuration]
instrument: example
verified: fixtures only
---

# Example instrument setup

A setup page gives the config entry with every key, the connection, and what the owner must have done before the first command. It never contains a real address or serial number: examples use the documentation range 192.0.2.0/24.

```json
{"instruments": {"gen": {"driver": "example", "tier": "read_config", "host": "192.0.2.10", "port": 5025,
                         "max_level_v": 1.0, "max_pulse_s": 2.0}}}
```

| Key | Default | Meaning |
|---|---|---|
| `host`, `port` | none, 5025 | where the instrument listens; `host` is required |
| `tier` | `read_only` | see [safety tiers](concepts/safety-tiers) |
| `max_level_v` | 1.0 | highest level any tool may set; 0 to 5 (the rating) exclusive of 0 |
| `max_pulse_s` | 2.0 | longest time the output may be on in one call; 0.1 to 60 |

Cabling: the output goes to a dummy load or a high-impedance meter input, never to a device under test until the owner says so. Start at the lowest level that shows on the meter. Switching the output on needs the `full` tier on this instrument.
