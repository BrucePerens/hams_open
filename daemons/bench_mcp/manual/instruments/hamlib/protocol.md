---
title: Hamlib protocol notes
summary: The rigctld commands used and where each fact came from.
see_also: [instruments/hamlib/limits, concepts/extending/provenance]
instrument: hamlib
verified: dummy rig only, 2026-10-29
---

# Hamlib protocol notes

Transport: TCP to `rigctld` on 127.0.0.1. Each command is sent as `+\<command> <arguments>` and newline-terminated; the "extended response" form answers with an echo line `<command>: <arguments>`, `Key: value` or bare value lines, and a final `RPRT <code>`. A negative code is an error (`-5` timeout, `-6` I/O, `-11` not available and so on; the module maps them to text and a next step). The commands used so far: `get_freq`, `get_mode`, `get_vfo`, `get_split_vfo`, `get_rit`, `get_xit`, `get_ptt`, `get_dcd`, `get_powerstat`, `get_info`, `get_mem`, `get_channel`, `get_level` and `get_func` (including `?` to list what the rig supports). The models list comes from running `rigctl -l` and `rigctl -m N -u`.

| Fact | Source kind | Source |
|---|---|---|
| command names, extended response form, return codes | observation of replies and the public documentation of rigctld's manual page | a Hamlib 4.6.2 rigctld started with the dummy rig, on the dev box, 2026-10-29 |
| STRENGTH is dB relative to S9 | other public documentation | Hamlib's description of the level |
| `tests/hamlib_fixtures/*.txt` | capture of a command's own output | `rigctl -l` and `rigctl -m 1 -u`, trimmed; no serial numbers, addresses or keys |

Not covered: `chk_vfo` and a few commands answer without an `RPRT` line, which the reader would wait on until the timeout; the module does not send them. `set_trn` and `get_trn` return "deprecated" on the dummy rig. Nothing in this module comes from Hamlib's source, only from its documented interfaces and observed behaviour; no Hamlib code is linked or copied.
