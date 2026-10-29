---
title: bench_hamlib_models
summary: Hamlib's supported models, filtered by words, or what one model can do. Needs no rig.
see_also: [instruments/hamlib/overview, instruments/hamlib/operation, instruments/hamlib/limits]
tools: [bench_hamlib_models]
instrument: hamlib
tier: read_only
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_models

Runs `rigctl -l` (the list) or `rigctl -m N -u` (one model). Use it to find the number for the `model` key of the config and to see which levels, functions, modes and ranges the backend declares before relying on them. It describes the backend, not a particular unit.

## Arguments

<!-- arguments -->

## Returns

List form: `total_models`, `matching`, `shown` and `models` (each `model`, `manufacturer`, `name`, `backend_version`, `status`). Model form: the name, `rig_type`, `ptt_type`, `dcd_type`, `port_type`, `can_read_levels`, `can_set_levels`, `can_read_functions`, `can_set_functions`, `modes`, `vfos`, `preamp_db`, `attenuator_db`, `agc_levels`, `receive_ranges_hz` and `transmit_ranges` (low and high in Hz, low and high power in watts).

## Failure modes

* `rigctl is not installed`: install the Hamlib utilities (`libhamlib-utils`).
* `rigctl does not know model N`: search the list.

## Example

`bench_hamlib_models {"search": "ic-7300"}` lists the matching models; `{"model": 1}` describes the dummy rig.
