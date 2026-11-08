---
title: bench_glasgow_prepare
summary: Glasgow: build and cache the gateware of a tool's applet ahead of its first use (no device, nothing driven).
see_also: [instruments/glasgow/limits, instruments/glasgow/operation, tools/bench_glasgow_safe_state]
tools: [bench_glasgow_prepare]
instrument: glasgow
tier: read_config
verified: fixtures only
---

# bench_glasgow_prepare

Runs `glasgow build` for the applet and pins a tool will use, at low priority, with its scratch file in /dev/shm. The cache key covers the applet, its pins and the hardware revision, so pass the same pin arguments you will give the tool. The first use of an applet configuration otherwise builds it (about 5 to 20 s on a 32-core machine) inside the tool's own timeout.

## Arguments

<!-- arguments -->

## Returns

`{"built": true, "applet": "uart", "revision": "C3", "build_s": 7.0, "cache_bytes": 600000}`.

## Failure modes

* `the build exited`: the toolchain (yosys, nextpnr) is part of the install; run `install_glasgow.py` again into a new directory if the install is damaged.
* `did not finish within N s`: raise `build_timeout_s` in the config.

## Example

`bench_glasgow_prepare {"tool": "i2c", "scl": "A0", "sda": "A1"}`
