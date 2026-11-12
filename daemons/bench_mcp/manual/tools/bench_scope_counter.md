---
title: bench_scope_counter
summary: The scope's hardware frequency counter, coarse and full precision.
see_also: [tools/bench_set, instruments/scope]
tools: [bench_scope_counter]
instrument: scope
tier: read_only
verified: fixtures only
---

# bench_scope_counter

Reads the scope's hardware frequency counter: `CYMT?` (the value in E-notation with its unit) and `CYMT_HW?` (full precision in Hz, kHz or MHz). The counter counts the trigger-level crossings of the trigger source (set the source with `bench_set`). Below 10 Hz the scope answers `10Hz` or `<10Hz`, and the tool then gives no hertz value. The guide lists no DVM or histogram function for this series, so none is offered; the real unit's reply header (the guide's text shows both `CYMOMETER` and `CYMT`) does not matter because only the value is read. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27. Reading only: nothing on the scope changes.

## Returns

`counter` and `full_precision` (the scope's text), `counter_hz` and `full_precision_hz` (numbers, or null when the text has no hertz value) and a `note`.

## Arguments

<!-- arguments -->


* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
