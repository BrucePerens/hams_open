---
title: bench_glasgow_uart_transact
summary: Glasgow: send bytes on a UART and collect the reply with limits on time, bytes and silence. Full tier, confirm to send, dry_run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_uart_transact]
instrument: glasgow
tier: full
verified: fixtures only
---

# bench_glasgow_uart_transact

8 data bits, parity and stop bits as given, any baud from 50 to 3 000 000. Sends (with `confirm`) then listens until `until_hex`, `idle_ms` of silence after data, `max_bytes` or `timeout_s` (at most the config's max_capture_s). A listen-only call needs no confirm.

## Arguments

<!-- arguments -->

## Returns

`{"sent": 3, "received": 4, "hex": "4f4b0d0a", "ended_by": "idle", "elapsed_s": 0.31, "baud_measured": 115200.0}`; `ended_by` is `until`, `idle`, `max_bytes` or `timeout`.

## Failure modes

As [bench_glasgow_i2c_scan](tools/bench_glasgow_i2c_scan). Nothing received is `received: 0, ended_by: timeout`; it is never turned into an invented reply.

## Example

`bench_glasgow_uart_transact {"baud": 115200, "send_text": "AT", "append": "cr", "confirm": true}`
