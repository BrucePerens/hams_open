---
title: Rotator protocol notes
summary: The rotctld commands used and where each fact came from.
see_also: [instruments/rotator/limits, instruments/hamlib/protocol, concepts/extending/provenance]
instrument: rotator
verified: dummy rotator only, 2026-10-29
---

# Rotator protocol notes

`rotctld` speaks the same text protocol as `rigctld`: a command written as `+\get_pos`, answered by an echo line, `Key: value` lines and a final `RPRT <n>`. Used: `get_pos` (Azimuth, Elevation), `set_pos <az> <el>`, `stop`, `park`, `dump_state` (Minimum and Maximum Azimuth and Elevation) and `get_info`. The client is the one of the hamlib module.

| Fact | Source kind | Source |
|---|---|---|
| command names, reply layout, return codes | observation of replies | a Hamlib 4.6.2 `rotctld -m 1` (dummy rotator) on the dev box, 2026-10-29 |
| the dummy rotator turns at about 6.6 degrees per second and answers `RPRT -21` for an elevation of 120 | observation | same |

Nothing in this module comes from Hamlib's source, only from its documented interfaces and observed behaviour; no Hamlib code is linked or copied.
