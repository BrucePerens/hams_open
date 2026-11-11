---
title: bench_radioctl_models
summary: The rigs radioctl has descriptions for, filtered by words, or what one rig declares. Needs no rig.
see_also: [instruments/radioctl/overview, instruments/radioctl/operation, instruments/radioctl/limits]
tools: [bench_radioctl_models]
instrument: radioctl
tier: read_only
verified: virtual radioctl rig only, 2026-11-11
---

# bench_radioctl_models

Reads the rig description files (`rigs/*.toml` in the config's `rigs_dir` or `RADIOCTL_RIGS`): the list, or one `rig`. Use it to find the id for the `rig` key of the config and to see what the description declares (protocol family and capabilities) before relying on it. It describes the description, not a particular unit.

## Arguments

<!-- arguments -->

## Returns

List form: `total_rigs`, `matching`, `shown` and `rigs` (each `rig`, `name`, `family`). Rig form: `rig`, `name`, `family` and `capabilities` (the names the description lists, such as `read-signal`, `set-ptt`, `split`).

## Failure modes

* `no rig description directory`: set `rigs_dir` in the rig's entry or `RADIOCTL_RIGS`.
* `no rig description "x"`: search the list.

## Example

`bench_radioctl_models {"search": "ic-7300"}` lists the matching rigs; `{"rig": "elecraft-kx3"}` describes the KX3.
