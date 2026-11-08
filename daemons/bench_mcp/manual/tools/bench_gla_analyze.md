---
title: bench_gla_analyze
summary: Logic analyzer: trigger window, sample rate, summary, glitch report, VCD, CSV and sigrok .sr files and UART, I2C or SPI decode of a VCD file already on disk. No device.
see_also: [tools/bench_gla_capture, instruments/glasgow/logic-analyzer]
tools: [bench_gla_analyze]
instrument: glasgow
tier: read_only
verified: fixtures only
---

# bench_gla_analyze

Works on a `.vcd` file in the output directory (`BENCH_MCP_OUT`): one written by `bench_glasgow_logic_capture` or `bench_gla_capture`, or any VCD of 1-bit wires (at most 16). The file name has no directory part. Output files are written next to it as `<name>_analysis.vcd`, `.csv` and `.sr` (`formats` chooses).

A trigger (`edge`, `level`, `pattern`) keeps `pre_trigger_ms` before it and `post_trigger_ms` after it (or `samples` at `sample_rate_hz`). With no trigger the whole file is kept. The sample rate defaults to the slowest of 1, 2, 5 steps with 4 samples on the shortest pulse; a rate you choose reports `missed_pulses`, the pulses that fall between samples. The summary has per pin the edges, frequency from the median rising-edge period, duty cycle, shortest and longest pulse and the glitches (pulses shorter than `glitch_ns`).

## Arguments

<!-- arguments -->

## Returns

`{"trigger": {...}, "window": {...}, "sampling": {"rate_hz", "samples", "missed_pulses"}, "summary": {"pins": {"A0": {"frequency_hz", "duty_cycle", "glitches": {"count", "first": [...]}, ...}}}, "files": {"vcd": {...}, "csv": {...}, "sr": {...}}, "decode": {...}}`. Decoders: UART (frames with parity and framing errors, text), I2C (address, R/W, data, ACKs, repeated start, stop), SPI (words per chip select).

## Failure modes

A file name with a directory part, another extension or no such file; a trigger that never occurs (the error says so); more than 8 million samples at the chosen rate; a decoder pin that is not in the file.

## Example

`bench_gla_analyze {"file": "glasgow_logic_1.vcd", "trigger": {"type": "edge", "pin": "A0", "edge": "fall"}, "pre_trigger_ms": 1, "decode": {"type": "uart", "pin": "A0", "baud": 9600}}`
