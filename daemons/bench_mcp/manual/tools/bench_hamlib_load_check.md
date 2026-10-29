---
title: bench_hamlib_load_check
summary: Dummy-load plausibility check, receive only: readings at the transmit frequency and reference stations against the baseline. Evidence, not proof.
see_also: [instruments/hamlib/limits, instruments/hamlib/measurement-recipes, tools/bench_hamlib_load_baseline]
tools: [bench_hamlib_load_check]
instrument: hamlib
tier: read_config
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_load_check

Run by hand to see whether the path looks like a dummy load, and run automatically before every tool that transmits. With the rig in receive on the supposed dummy-load path it reads the S-meter (several times, with the spread), the squelch and the audio at the transmit frequency and at reference frequencies chosen from the [station table](tools/bench_hamlib_stations) for that band and time of day, with the preamp on and the attenuator off, and compares them with the baseline recorded by [bench_hamlib_load_baseline](tools/bench_hamlib_load_baseline). The rig's frequency, mode, AF, preamp and attenuator are put back. It never transmits.

It REFUSES when: any reading is more than `margin_db` plus the spread over the baseline; a time station's ticks or tones are heard in the audio (a real antenna hears these; a load does not, and the ticks show well below what the S-meter reads); the squelch opens where it was closed; the audio is louder than at the baseline; a steady carrier note appears that was not there; the readings are too unsteady to compare; the S-meter cannot be read; the audio input is dead (all zeros) or cannot be recorded; the rig is keyed; the baseline was recorded with other settings or covers too few of these frequencies. Without a baseline it reports `no_baseline`, and the transmit tools require the owner to record one. Hearing no station is not a pass or a fail by itself: propagation can fail on all of them.

**This is evidence, not proof.** An open or unplugged connection receives nothing and looks like a good load (the low-power SWR probe of the transmit tools exists for that); an antenna on a dead band at low gain also hears nothing; the baseline is valid only for the same cabling and rig settings and must be recorded again after any change; and the owner's attestation in the config still gates transmitting.

## Arguments

<!-- arguments -->

## Returns

`verdict` (`consistent_with_a_load`, `refuse` or `no_baseline`), `reasons`, `readings` (per frequency: strength, spread, squelch, audio findings), `baseline` (date, who, cabling), `compared_with_baseline`, `stations_heard_now` (and the table's `last_heard` is updated), `audio` (the input used, or that none is configured) and the note above.

## Failure modes

* `no transmit attestation`: the rig has no `transmit` block; nothing to check.
* `outside the frequency ranges the owner listed`: the check is only for the owner's listed ranges.
* `the S-meter cannot be read` and `the rig's audio cannot be recorded`: errors, never a pass.

## Example

`bench_hamlib_load_check {"frequency_hz": 7050000}`.
