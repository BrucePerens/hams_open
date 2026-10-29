---
title: bench_vna_sweep
summary: Linear VNA sweep measuring S21 and S11 as raw ratios, optionally corrected with a stored calibration; saves a CSV.
see_also: [tools/bench_vna_calibrate_measure, tools/bench_vna_calibrate_build, tools/bench_vna_power_cycle, instruments/vna/monitoring-and-recovery, instruments/vna/limits]
tools: [bench_vna_sweep]
instrument: vna
tier: read_config
verified: code only, 2026-10-29
---

# bench_vna_sweep

Sweeps `start_hz` to `stop_hz` in `points` points with RF on the TX port and measures S21 (RX channel) and S11 (reflection channel). Each point takes about 0.2 s at 60 ms per point, and the unit emits for six point times. The values are raw ratios of the unit's own channels: the cable, bridge and front end are not removed unless a calibration is applied.

## Arguments

<!-- arguments -->

* `instrument`: the VNA alias; optional when only one VNA is configured.
* `start_hz`: first frequency in hertz, above 0 and at most 1.3 GHz. Required.
* `stop_hz`: last frequency in hertz; default `start_hz` (a single point).
* `points`: whole number from 1 to 401, default 1.
* `if_hz`: intermediate frequency of the measurement, a divisor of 48 kHz (`12000`, `6000`, `4000`); lower means less noise and a longer point. Default from the instrument's configuration.
* `point_ms`: measurement time per point in milliseconds, 0.2 to 100; the configured default is 60.
* `rf_level`: output level setting, a whole number from 0 to 16383 (16383 is the top level, about -17 dBm).
* `calibration`: path of a `.cal` file written by [bench_vna_calibrate_build](tools/bench_vna_calibrate_build).
* `cable`: required with `calibration`: exactly the text given when the standards were measured. A calibration is refused at another IF, time per point, RF level or cable text.

## Returns

`points`, `start_hz`, `stop_hz`, `if_hz`, `calibrated`, `s21_db_min`, `s21_db_max`, `file` (CSV: `freq_hz`, S21 and S11 as real, imaginary, dB and degrees), `rows` (the points themselves when there are 51 or fewer: `freq_hz`, `s21_db`, `s21_deg`, `s11_db`, `s11_deg`), `recovery` (counts of re-measured points, transfer resends, power cycles and audio overruns), `recovered_failures` (the last five failed attempts that were cured), `temperature_counts_after`, `max_if_error_hz`, `max_level_change`. With a calibration also `calibrated_file` and `calibrated_rows`.

## Failure modes

Argument errors name the argument and range. `no signal on the reference channel(s)`: if the message says all four channels are dead, check the TX connector, the pad, the tee and the leads first (a loose TX SMA looks exactly like this); if the measured channels have signal it is the silent state: call [bench_vna_power_cycle](tools/bench_vna_power_cycle). A point that fails repeatedly causes one automatic cold reset through the hub, then the call fails listing every attempt; see [errors](concepts/errors). A running generator or meter must be stopped first.

## Example

Loopback check, TX cabled to RX: `bench_vna_sweep {"start_hz": 1e6, "stop_hz": 300e6, "points": 11}`. A cable between TX and RX is not 0 dB raw: about -1 dB at 10 MHz rising to +2.6 dB at 300 MHz with a phase linear in frequency (measured 2026-10-28, see the VNA pages).
