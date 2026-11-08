---
title: bench_gla_capture
summary: Logic analyzer: records pins through the Glasgow backend, then applies a host-side trigger, pre-trigger window and sample rate, writes VCD, CSV and .sr files and decodes UART, I2C, SPI.
see_also: [tools/bench_gla_analyze, tools/bench_glasgow_logic_capture, instruments/glasgow/logic-analyzer]
tools: [bench_gla_capture]
instrument: glasgow
tier: read_config
verified: fixtures only
---

# bench_gla_capture

Passive: every pin is an input. It asks `bench_glasgow_logic_capture` for `duration_s` seconds (0.1 up to the config's `max_capture_s`), so that tool's rules apply: the port voltage must have been set by `bench_glasgow_port_set`, the link must be 480 Mb/s, and the pins are parked afterwards. The revision C analyzer has no trigger and no sample-rate setting: the recording has every change with a 20.8 ns timestamp, and the trigger, window, rate and decode are applied to that recording here. A trigger that comes late in a long capture still has its pre-trigger part, as long as it was inside the recorded duration.

Arguments and results are those of [bench_gla_analyze](tools/bench_gla_analyze), plus `pins`, `duration_s` and `pull`. The answer also holds `backend` (the capture file of the backend and whether its FIFO overran) and the per-edge byte `limits`.

## Arguments

<!-- arguments -->

`formats` is a list of the files to write: `vcd`, `csv`, `sr` (default all three; an empty list writes none).

## Returns

As bench_gla_analyze; pin names are the port pins (`A0`...).

## Failure modes

The backend's: port voltage not set, link at full speed, applet failure; `overrun: true` means the link could not keep up and the capture ends there (see [bench_gla_limits](tools/bench_gla_limits)).

## Example

`bench_gla_capture {"pins": "A0:1", "duration_s": 2, "trigger": {"type": "edge", "pin": "A0", "edge": "rise"}, "pre_trigger_ms": 5, "post_trigger_ms": 50, "decode": {"type": "uart", "pin": "A1", "baud": 115200}}`
