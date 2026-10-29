---
title: bench_hamlib_tx_power_sweep
summary: Transmit steps of rising power into the attested dummy load, each its own keyed interval with its own watchdog. All the gates of the key test apply.
see_also: [instruments/hamlib/limits, procedures/test-a-transmitter-into-a-dummy-load, instruments/hamlib/setup]
tools: [bench_hamlib_tx_power_sweep]
instrument: hamlib
tier: full
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_tx_power_sweep

**Radiates if the load is not there; the software cannot know what is connected.** Everything in [bench_hamlib_tx_key_test](tools/bench_hamlib_tx_key_test) applies: the attestation, the owner's ranges, modes and caps, `confirm_dummy_load`, `reason`, the load plausibility check, the optional alert, the low-power probe, the watchdog and the log. `powers_w` lists 1 to 8 rising power levels in watts, each at most the cap; each is keyed for `dwell_s` seconds as a separate interval (with its own watchdog and its own begin and end log lines), the meters are read, the rig is released and read back, and then the next step starts. The sweep stops at the first step that is aborted (SWR over the limit, a power meter far over the cap, a meter that cannot be read). The total keyed time is at most 60 s. The old power setting, mode and frequency are restored.

## Arguments

<!-- arguments -->

## Returns

`steps`: for each level `power_w`, `setting`, `meters`, `aborted` and `ptt_read_back_off`; plus `plan`, `load_check`, `operator_alert`, `swr_probe`, `rig_settings_not_restored` and `keyed_interval_log`. With `dry_run`, the plan and the total keyed time.

## Failure modes

As for the key test. `powers_w must rise` and `the most in one call is 60 s` are refused before anything is sent.

## Example

`bench_hamlib_tx_power_sweep {"frequency_hz": 7050000, "powers_w": [2, 5, 10], "dwell_s": 1, "reason": "K6BP: PA linearity", "confirm_dummy_load": true}`.
