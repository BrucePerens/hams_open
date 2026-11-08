---
title: bench_glasgow_logic_capture
summary: Glasgow: logic-analyzer capture of 1 to 16 pins to a VCD file with a per-pin summary. Bounded in time and size.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_logic_capture]
instrument: glasgow
tier: read_config
verified: fixtures only
---

# bench_glasgow_logic_capture

Runs the `analyzer` applet (revC and later: 48 MHz sample clock, no trigger, no sample-rate setting) for `duration_s` and writes a VCD with a 1 ns timescale. The summary counts transitions, the fraction of time high, the shortest and longest pulse, the rising-edge frequency, first and last level, and unknown samples (the FIFO overrun marker). `pull` turns on weak pull-ups or pull-downs on every captured pin.

## Arguments

<!-- arguments -->

## Returns

`{"file": ".../glasgow_logic_<ms>.vcd", "file_bytes": 1234, "summary": {"duration_ns": 5000000, "pins": {"clk": {"transitions": 10, "high_fraction": 0.5, "min_pulse_ns": 500000, "rising_edge_frequency_hz": 1000.0, ...}}}, "overrun": false, "stopped_by": "duration or size limit"}`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan). `overrun: true`: the USB link could not keep up; the capture ends at the overrun.

## Example

`bench_glasgow_logic_capture {"pins": "A0:3", "duration_s": 2, "names": "clk,mosi,miso,cs"}`
