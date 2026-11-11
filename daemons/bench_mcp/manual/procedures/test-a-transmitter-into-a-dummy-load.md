---
title: Test a transmitter into a dummy load
summary: The order of steps for a transmit test with a radioctl rig: attestation, baseline, dry run, test, safe state; and what to do when anything looks wrong.
see_also: [instruments/radioctl/limits, instruments/radioctl/setup, tools/bench_radioctl_tx_key_test, tools/bench_radioctl_safe_state]
tools: [bench_radioctl_stations, bench_radioctl_load_baseline, bench_radioctl_load_check, bench_radioctl_tx_key_test, bench_radioctl_tx_power_sweep, bench_radioctl_safe_state]
instrument: radioctl
verified: virtual radioctl rig only, 2026-11-11
---

# Test a transmitter into a dummy load

The software cannot know what is connected to the rig. These steps are for the owner or an agent working with the owner at the bench; the person must see the dummy load on the antenna port before step 3.

1. The rig's config entry has the `transmit` block (see [setup](instruments/radioctl/setup)): the owner's name, the date, the cabling, the caps, the frequency ranges and the allowed modes. The owner is answerable for the ranges. Without it the transmit tools do not exist.
2. `bench_radioctl_stations`: review the reference stations for the band and retire the ones that are off the air (`bench_radioctl_station_set`). Silence from a station is not a fault, and hearing none proves nothing.
3. With the dummy load connected: `bench_radioctl_load_baseline {"frequency_hz": ..., "dry_run": true}` shows the plan, then the same with `"confirm_dummy_load": true` from the person at the bench. It refuses if it hears a time station: then something is on the antenna port. Record it again after any change of cabling, load or rig settings.
4. `bench_radioctl_load_check {"frequency_hz": ...}`: it should say `consistent_with_a_load`. That is evidence, not proof.
5. `bench_radioctl_tx_key_test` with `"dry_run": true`, read the plan, then with `confirm_dummy_load` and a `reason`, a short time and a low power first. Read `swr_probe`, `meters` and `aborted`.
6. For several levels: `bench_radioctl_tx_power_sweep`, the same gates, one keyed interval per step.
7. If anything looks wrong, at any time: `bench_radioctl_safe_state`. If it says SAFE STATE NOT CONFIRMED, switch the rig off at its front panel or pull its antenna feed or power, and ask the owner to look at it.
8. The keyed-interval log in the managed directory lists every interval (and refusal) with its time, frequency, power, duration and reason.
