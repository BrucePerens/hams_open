---
title: bench_hamlib_load_baseline
summary: Record the load-check baseline for a frequency range: the readings through the dummy load, stored with the attestation. Needs confirm_dummy_load; dry_run.
see_also: [instruments/hamlib/limits, instruments/hamlib/measurement-recipes, tools/bench_hamlib_load_check]
tools: [bench_hamlib_load_baseline]
instrument: hamlib
tier: full
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_load_baseline

Records what the receiver hears through the dummy load, as the reference for [bench_hamlib_load_check](tools/bench_hamlib_load_check). `confirm_dummy_load` must be true and must come from the person at the bench: it states that the rig's antenna port is on the dummy load that the config's `cabling` describes. The tool cannot know that. It then measures exactly as the check does, and refuses to record if a time station is heard, the audio is dead, the S-meter cannot be read or the readings are unsteady; otherwise it stores them for the owner's frequency range that holds `frequency_hz`, with the attestation's name, the date and the cabling. Record it again after any change of cabling, load, rig settings or the audio path. `dry_run` lists the plan and does nothing at all. Receive only; it never keys.

## Arguments

<!-- arguments -->

## Returns

`recorded`, the range, `taken_on`, `attested_by`, `cabling`, the readings, and `rig_settings_not_restored`. With `dry_run`: `{"dry_run": true, "sent": [], "would": [...]}`.

## Failure modes

* `call it with confirm_dummy_load: true`: nothing was measured or stored.
* `no baseline was recorded: ...`: the reasons are listed. A load hears nothing of a time station; find out what is on the antenna port.
* `outside the frequency ranges the owner listed`.

## Example

`bench_hamlib_load_baseline {"frequency_hz": 7050000, "dry_run": true}`, then the same with `"confirm_dummy_load": true`.
