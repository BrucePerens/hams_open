---
title: SSA3000X setup
summary: Config entry for the analyzer, the LAN address and port, and the connections for a spectrum and for an S21 measurement.
see_also: [instruments/sa/overview, concepts/configuration]
instrument: sa
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# SSA3000X setup

```json
{"instruments": {"sa": {"driver": "siglent_ssa", "tier": "read_config", "host": "192.0.2.42", "port": 5025}}}
```

| Key | Default | Meaning |
|---|---|---|
| `host`, `port` | none, 5025 | the analyzer's LAN address and its raw SCPI socket; `host` is required |
| `tier` | `read_only` | `read_only` lists status and screenshot; `read_config` adds sweeps, markers and the generator; see [safety tiers](concepts/safety-tiers) |
| `max_tg_dbm` | 0 | optional owner cap on the generator level, -20 to 0 dBm; the unit's own range is -20 to 0 dBm, so the default adds no cap |

Give the analyzer a fixed address (System > Interface > LAN on the front panel) and connect it to the bench network. The raw socket is port 5025; the unit also answers VXI-11 and USBTMC, which this driver does not use (USB replies are cut short on this unit).

Connections: for a spectrum, the signal goes to the RF INPUT (50 ohm, 20 dB attenuator by default; maximum input is printed on the connector). For an S21 measurement, the TG OUTPUT goes through the device under test to the RF INPUT; take the through reference with a straight cable between them first.
