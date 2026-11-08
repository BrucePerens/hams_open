---
title: Glasgow logic analyzer and pattern generator
summary: How the bench_gla tools work on the revision C analyzer (no hardware trigger, 48 MHz timestamps), what the USB link limits, and what has and has not been measured.
see_also: [instruments/glasgow/limits, instruments/glasgow/logic-bench-session, tools/bench_gla_capture, tools/bench_gla_pattern, tools/bench_gla_limits]
instrument: glasgow
verified: full-speed rate measured on the unit 2026-11-08; no capture has run yet (the unit links at 12 Mb/s); everything else fixtures only
---

# Glasgow logic analyzer and pattern generator

`bench_gla_capture`, `bench_gla_analyze`, `bench_gla_limits` and `bench_gla_pattern` sit on top of the Glasgow tools `bench_glasgow_logic_capture` and `bench_glasgow_pattern`. They add no way to reach the device: the port voltage, tier, time limit, confirm and park rules are those tools'.

## What the revision C analyzer is

The `analyzer` applet writes one record for each change of the captured pins (1 to 16 of them): the time since the previous change in 48 MHz ticks (20.8 ns), and the new pin values. There is no sample-rate register and no trigger. So the tools record continuously for `duration_s` and do the rest on the host: the trigger (edge, level held for a time, or a 0/1/x pattern across the pins) is searched in the recording, the pre-trigger buffer is the part before it, and `sample_rate_hz` resamples the changes onto a fixed grid (needed for the sigrok `.sr` file). A rate below 48 MHz can hide short pulses: the answer counts them (`missed_pulses`) and the automatic rate puts 4 samples on the shortest pulse.

## What the USB link limits

The limit is edges per second, not the sample rate: a record is 3 to 6 bytes (see [bench_gla_limits](tools/bench_gla_limits)). A quiet or slow signal can be recorded for the full `max_capture_s`; a fast clock fills the link, the device's FIFO overruns and the capture ends at the overrun (`overrun: true`).

| Link | Measured or modelled | Edges per second (4 pins, closely spaced) |
|---|---|---|
| 12 Mb/s | measured: `glasgow run benchmark` sink mode 0.91 MiB/s; the IN direction fails with `LIBUSB_TRANSFER_OVERFLOW`, so a capture cannot run at all | would be about 318 000 if it could |
| 480 Mb/s | model: about 40 MB/s | about 13 million; not measured on this unit yet |

High sample or edge rates need the 480 Mb/s link. Until the unit links at high speed, the only things that work are the register-only tools: `bench_gla_pattern`, `bench_gla_analyze` on existing files, and `bench_gla_limits`.

## Files

VCD (1 ns timescale; opens in GTKWave and PulseView), CSV (`time_ns` and one column per pin, one row per change) and sigrok `.sr` (a ZIP with `version`, `metadata` and packed samples; opens in PulseView and sigrok-cli) are written to the output directory. The `.sr` file is not compressed; at 48 MHz it is 6 MB per second per 8 pins, so lower `sample_rate_hz` for long windows (the limit is 8 million samples).

## Pattern generator

Host-timed through control-gpio: a hold is 0.5 ms or more and the edges have about 1 ms of jitter, so a square wave tops out near 1 kHz. The planner reports the frequency and duty that will really come out. It cannot be looped back into the analyzer on the same Glasgow: one applet runs at a time. Check a pattern with the scope ([logic-bench-session](instruments/glasgow/logic-bench-session)).
