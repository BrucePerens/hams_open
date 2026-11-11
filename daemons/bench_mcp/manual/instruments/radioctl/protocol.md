---
title: radioctl protocol notes
summary: The radioctld commands used and where each fact came from.
see_also: [instruments/radioctl/limits, concepts/extending/provenance]
instrument: radioctl
verified: virtual radioctl rig only, 2026-11-11
---

# radioctl protocol notes

Transport: TCP to `radioctld` on 127.0.0.1. Each command is sent as `+\<command> <arguments>` and newline-terminated; the "extended response" form answers with an echo line `<command>: <arguments>`, `Key: value` or bare value lines, and a final `RPRT <code>`. A negative code is an error (`-5` timeout, `-6` I/O, `-11` not available and so on; the module maps them to text and a next step). The commands used: `get_freq`, `get_mode`, `get_vfo`, `get_split_vfo`, `get_rit`, `get_xit`, `get_ptt`, `get_dcd`, `get_powerstat`, `get_info`, `get_mem`, `get_channel`, `get_level` and `get_func` (a `?` after them is tried to list what the rig supports; radioctld answers it with `RPRT -1`, and the tools then ask each name in turn). Also used: `dump_state` (the rig's receive and transmit ranges and the level ranges; the reply is free text that ends with a `done` line and has no `RPRT`), `set_ptt`, `set_freq`, `set_mode`, `set_level` and the levels `RFPOWER`, `SWR`, `ALC`, `TEMP_METER` and `RFPOWER_METER_WATTS`. radioctld has no `mW2power` or `power2mW`: the tools take the power setting as a fraction of the highest power of the transmit range in `dump_state`, which for a managed rig is the owner's `full_power_w`. The rig list comes from reading the rig description files, not from the daemon.

| Fact | Source kind | Source |
|---|---|---|
| command names, extended response form, return codes | radioctl's own documentation (`daemons/radioctl/docs/manual/C-radioctld-command-reference.md`) and observation of replies | radioctld built from this repository with virtual rigs, 2026-11-11 (the first version was written against Hamlib's `rigctld`, 2026-10-29) |
| STRENGTH is dB relative to S9, Elecraft rigs only | radioctl's command reference | same |
| rig identity and capabilities | the description files themselves | `daemons/radioctl/rigs/*.toml` |

Observed on radioctld with virtual rigs: `dump_state` ends with `done` and no `RPRT`; `get_level ?` is refused (`RPRT -1`); memory channels, `get_dcd` and the functions other than RIT and XIT are not served (`RPRT -4` or `-11`); a rig keyed by one connection is unkeyed when that connection closes, and a second connection cannot key it; a virtual KX3 reads `STRENGTH` -54 dB, an IC-7300 has none. The transmit watchdog process has a connection of its own and unkeys over it (an unkey is open to every client). Nothing in this module comes from another program's source, only from the daemon's documented interfaces and observed behaviour.
