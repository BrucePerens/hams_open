---
title: bench_gla_limits
summary: Logic analyzer: what the USB link and the revision C analyzer can carry, as a model, with the measured limits on the instrument page.
see_also: [instruments/glasgow/logic-analyzer, tools/bench_gla_capture]
tools: [bench_gla_limits]
instrument: glasgow
tier: read_only
verified: model; 12 Mb/s rate measured on the unit 2026-11-08, 480 Mb/s not measured
---

# bench_gla_limits

The analyzer sends one record per pin change: 1 to 3 bytes of time since the last change (7 bits each, 48 MHz ticks), one event byte, and one data byte per 8 pins. The tool divides the link's usable bulk rate by that size. At 12 Mb/s (full speed) the device's gateware cannot stream at all, so `capture_works` is false. `link_mbps` defaults to the speed the kernel reports for the Glasgow, else 480.

## Arguments

<!-- arguments -->

`pins` is how many pins are captured (1 to 16, default 4); it sets the data bytes of each record.

## Returns

`{"model": true, "link_mbps": 480, "capture_works": true, "rows": [{"spacing": "...", "bytes_per_edge": 3, "max_edges_per_s": 13333333}], "trigger": "...", "sample_rate": "..."}`. The numbers are a model, not a measurement.

## Failure modes

Only the instrument and tier errors of any tool; it drives nothing.

## Example

`bench_gla_limits {"pins": 4}`
