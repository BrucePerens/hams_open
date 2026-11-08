---
title: bench_glasgow_uart_sniff
summary: Glasgow: passively listen on two UART lines for a bounded time and return the bytes per channel.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_uart_sniff]
instrument: glasgow
tier: read_config
verified: fixtures only
---

# bench_glasgow_uart_sniff

Both pins are inputs; the pin only says which channel the bytes belong to. The capture goes to a CSV in the output directory, stopped by SIGINT at `duration_s` or when the file passes max_capture_mb.

## Arguments

<!-- arguments -->

## Returns

`{"file": ".../glasgow_uart_<ms>.csv", "channels": {"rx": {"bytes": 9, "hex": "...", "text": "...", "frame_errors": 0, "parity_errors": 0}, "tx": {...}}, "stopped_by": "duration or size limit"}`. Hex and text are cut at 4096 bytes per channel (`truncated`); the file has everything.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan).

## Example

`bench_glasgow_uart_sniff {"baud": 9600, "duration_s": 5}`
