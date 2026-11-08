---
title: bench_glasgow_applets
summary: Glasgow: the applets and pin roles the tools use, and what the server will never run.
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_applets]
instrument: glasgow
tier: read_only
verified: fixtures only
---

# bench_glasgow_applets

A static list for the agent: which tool uses which Glasgow applet, with the default pins, and the things deliberately not provided (flash, factory, other applets, command lines).

## Arguments

<!-- arguments -->

## Returns

`{"applets": [{"tool": "...", "applet": "i2c-controller", "pins": {"scl": "A0", "sda": "A1"}}, ...], "not_provided": [...], "ports": ["A", "B"]}`.

## Failure modes

None: it opens no device.

## Example

`bench_glasgow_applets {}`
