---
title: bench_hamlib_tx_key_test
summary: Transmit test into the owner's attested dummy load: keys the rig for a bounded time at a bounded power and reads the meters. dry_run, confirm_dummy_load, watchdog.
see_also: [instruments/hamlib/limits, procedures/test-a-transmitter-into-a-dummy-load, instruments/hamlib/setup]
tools: [bench_hamlib_tx_key_test]
instrument: hamlib
tier: full
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_tx_key_test

**This tool radiates if the dummy load is not what the owner says it is. The software cannot know what is connected to the rig.** The owner's attestation in the config (the `transmit` block: `dummy_load: true`, name, date, cabling, `max_power_w`, `max_seconds`, `frequency_ranges`, `allowed_modes`) is the control. The tool exists only for a rig with that block and at tier `full`.

It keys the rig on `frequency_hz` in `mode` at `power_w` for `seconds`, reads power, SWR, ALC and temperature meters every `sample_every_ms` (those the rig reports), releases PTT and reads it back. In order:

1. Arguments against the attestation: the frequency must be inside the owner's ranges (there is no default list: the owner lists the frequencies they may use and answers for their licence), the mode allowed, the power at most the cap, the time at most `max_seconds` and 60 s. Then the rig: inside its own transmit range, not already keyed, split off and XIT 0 (both would move the transmit frequency), the power setting not above the cap (unless the owner allowed the tool to lower it, and then the old setting is restored), the lowest power setting known. Nothing is changed so far.
2. `dry_run: true` returns the plan and stops. Without `confirm_dummy_load: true`, from the person at the bench, it refuses. `reason` (who and why, up to 120 characters) is required and goes into the log.
3. The [load plausibility check](tools/bench_hamlib_load_check) must say "consistent with a load"; with no baseline, or any sign of an antenna-like path, it refuses.
4. The operator alert, if the owner configured `operator_alert`: if it cannot be heard, nothing is sent.
5. Frequency, mode and power are set and read back. The low-power probe keys the rig at its lowest power for `swr_probe_s` and reads the SWR: an open or shorted connection receives nothing, so only the SWR shows it. An SWR that is over `max_swr` or is not reported refuses (the owner may turn the probe off with `swr_probe: false` and accepts that risk).
6. The watchdog process is started and must report armed. Then the keyed interval, with the meters read; it stops at once if SWR passes the limit or the power meter reads far over the cap, if a meter cannot be read, or if the rig is released by something else.
7. Release, read-back, power, mode and frequency restored, and a begin and an end line written to the keyed-interval log in the managed directory (time, frequency, mode, power, duration, who and why, outcome).

The watchdog is a separate process. It un-keys the rig and sets the minimum power when the deadline passes, when the server dies, is killed or exits, or when the tool aborts. See [limits](instruments/hamlib/limits) for what that does not cover.

## Arguments

<!-- arguments -->

## Returns

`plan`, `load_check`, `operator_alert`, `swr_probe`, and `keyed`: `seconds`, `aborted` (why it ended early, or null), `ptt_read_back_off`, `meters` (min, max and mean of power in watts, SWR, ALC and temperature; `not_reported` names the ones the rig lacks) and `samples`. `rig_settings_not_restored` lists anything that could not be put back; `keyed_interval_log` is the log file.

## Failure modes

* Every refusal above starts the message with what was refused and ends with `Nothing was sent` or the next action.
* `RELEASE NOT CONFIRMED`: the rig could not be shown to be un-keyed. Switch it off at the rig now. The watchdog was left to un-key it and set the minimum power; call [bench_hamlib_safe_state](tools/bench_hamlib_safe_state).
* `the transmit watchdog did not report armed`: nothing was keyed.

## Example

`bench_hamlib_tx_key_test {"frequency_hz": 7050000, "mode": "USB", "power_w": 5, "seconds": 2, "reason": "K6BP: PA check into the load", "confirm_dummy_load": true}`; first with `"dry_run": true`.
