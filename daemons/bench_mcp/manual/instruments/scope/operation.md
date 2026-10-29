---
title: Scope operation
summary: Step-by-step procedures for every scope function.
see_also: [instruments/scope/limits, instruments/scope/measurement-recipes, tools/bench_set]
tools: [bench_status, bench_measure, bench_capture, bench_scope_spectrum, bench_screenshot, bench_set, bench_scope_self_cal]
instrument: scope
verified: probed on the unit (SDS1202X-E, firmware 1.3.27), 2026-10-28
---

# Scope operation

## Look before you change

1. `bench_status {"instrument": "scope"}`: trigger mode, timebase, per-channel volts per division, offset, coupling, probe ratio.
2. Note the values you will change so you can restore them.

## Show a signal

1. `bench_set {"channel": 1, "coupling": "D1M", "attenuation": 1, "vdiv_v": 0.1, "timebase_s": 1e-6, "trigger_mode": "AUTO"}`.
2. `bench_measure {"channel": 1}` gives peak-to-peak, mean, frequency and duty cycle. `****` means no valid measurement: adjust the scale so the signal fills 2 to 6 divisions and a few periods show.

## Measure parameters

`bench_measure` takes any of PKPK, MAX, MIN, AMPL, TOP, BASE, CMEAN, MEAN, RMS, CRMS, OVSN, FPRE, OVSP, RPRE, PER, FREQ, PWID, NWID, RISE, FALL, WID, DUTY, NDUTY. The scope's counter is wrong above about 150 MHz.

## Capture a waveform

`bench_capture {"channel": 1}` needs a running scope (`AUTO` or `NORM`); a stopped scope is refused unless `allow_stopped`. To catch one event, set `trigger_mode` `SINGLE`, wait for the trigger, then `bench_capture` with `allow_stopped: true`. The file is little-endian f32 volts; the sample interval is in the result.

## Spectrum of a tone

`bench_scope_spectrum {"channel": 1, "window": "hanning", "points": 131072}` after setting a vertical scale where the signal uses most of the screen (8-bit samples: lines more than about 45 dB below the strongest are not reliable). Use `timebase_s` to trade span for resolution: bin width is sample rate over `points`.

## Triggers

`bench_set` with `trigger_type` selects EDGE, SLEW, GLIT, INTV, RUNT or TV on the channel; the type's thresholds keep the scope's own values. Window, pattern, drop-out and serial-bus triggers were not accepted.

## Screenshots and self-calibration

`bench_screenshot` saves the display. `bench_scope_self_cal` is for the person at the bench only: all probes and cables off, `confirm_inputs_disconnected: true`; the vertical settings may be reset (one run changed 1 V/div to 20 mV/div), the calibration runs a minute or two.

## Leave it as found

Restore the settings from step 2 and trigger mode `AUTO`.
