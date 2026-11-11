---
title: Rotator protocol notes
summary: The commands of the rotator listener used and where each fact came from.
see_also: [instruments/rotator/limits, instruments/radioctl/protocol, concepts/extending/provenance]
instrument: rotator
verified: virtual rotator only, 2026-11-11
---

# Rotator protocol notes

radioctld's rotator listener speaks the `rotctld` text protocol, the same line protocol as its rig listeners: a command written as `+\get_pos`, answered by an echo line, `Key: value` lines and a final `RPRT <n>`. Used: `get_pos` (Azimuth, Elevation), `set_pos <az> <el>`, `stop`, `park`, `dump_state` (the `min_az=`, `max_az=`, `min_el=` and `max_el=` lines, read to its `done` line; the older `Minimum Azimuth:` form is also read) and `get_info`. The client is the one of the radioctl module.

| Fact | Source kind | Source |
|---|---|---|
| command names, reply layout, return codes | observation of replies | radioctld with a virtual Yaesu G-5500 rotator, built from this repository, 2026-11-11 (the first version was written against Hamlib's own rotctld, 2026-10-29) |
| the virtual rotator's limits are azimuth 0 to 450 and elevation 0 to 180; a position outside them is refused with `RPRT -1` | observation and radioctl's manual, chapter 9 | same |

Nothing in this module comes from another program's source, only from the daemon's documented interfaces and observed behaviour.
