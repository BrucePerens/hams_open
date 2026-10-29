---
title: bench_capture
summary: Read a scope channel's waveform memory as volts, save it as float32 and return a summary.
see_also: [tools/bench_scope_spectrum, tools/bench_set, instruments/scope]
tools: [bench_capture]
instrument: scope
tier: read_only
verified: code only, 2026-10-29
---

# bench_capture

Downloads the channel's waveform memory (every point, no decimation), scales the 8-bit samples to volts using the channel's volts per division and offset, writes the result to the output directory and returns statistics.

## Arguments

<!-- arguments -->

* `instrument`: the scope alias; optional when only one scope is configured.
* `channel`: `1` or `2`.
* `allow_stopped`: by default a scope in `STOP` is refused, because its memory is an old frozen frame. Pass `true` only when you stopped it on purpose to capture a single event; otherwise set `trigger_mode` `AUTO` with [bench_set](tools/bench_set) first.

## Returns

`channel`, `samples`, `sample_interval_s`, `acquisition_was_stopped`, `min_v`, `max_v`, `mean_v`, `pkpk_v`, `rising_edges`, `estimated_frequency_hz` (from the edges of the memory), `file` (path) and `file_format` (little-endian f32 volts). The number of samples is the scope's memory depth (14 kpoints by default on the tested unit).

## Failure modes

`the scope is stopped, so its waveform memory is an old frozen frame` (see `allow_stopped`); `empty waveform`; a timeout when the scope is busy. The transfer takes up to a few seconds for deep memory.

## Example

`bench_capture {"channel": 1}` returns the summary and a path such as `<output dir>/ch1_<time stamp>.f32`; read it with any tool that reads raw little-endian float32.
