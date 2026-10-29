---
title: Example instrument protocol notes
summary: The command subset and where each fact came from. The model for a protocol and provenance page.
see_also: [instruments/example/limits]
instrument: example
verified: fixtures only
---

# Example instrument protocol notes

SCPI over TCP port 5025, one command per line. Used: `*IDN?`, `OUTP?`, `OUTP ON|OFF`, `OUTP:TMO <seconds>`, `LEV?`, `LEV <volts>`, `MEAS:VOLT?`.

Provenance (one row per source, with its kind; kinds are: vendor programming manual, observation of the device's replies, capture of the owner's own session, measurement, other public documentation):

| Fact | Source kind | Source |
|---|---|---|
| command names and reply format | invented for this template | there is no real instrument |
| `tests/example_fixtures/status.txt` | recorded-session format example | scrubbed: serial `SN-0000`, no address |

A protocol page for a real instrument has this table filled with real sources and a sentence stating that nothing was copied from vendor source, firmware, drivers or help text.
