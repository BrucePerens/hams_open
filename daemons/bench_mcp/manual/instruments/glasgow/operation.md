---
title: Glasgow operation
summary: The normal order of calls, one session at a time, and examples.
see_also: [instruments/glasgow/overview, instruments/glasgow/bench-session, tools/bench_glasgow_port_set]
instrument: glasgow
verified: fixtures only
---

# Glasgow operation

1. [bench_glasgow_list](tools/bench_glasgow_list): is it there, at high speed, is the install present?
2. [bench_glasgow_status](tools/bench_glasgow_status): ports at 0 V, no fault.
3. Optional: [bench_glasgow_prepare](tools/bench_glasgow_prepare) for the pins you will use.
4. With the person at the bench saying the target's I/O voltage: [bench_glasgow_port_set](tools/bench_glasgow_port_set) with `confirm: true`. Use `dry_run: true` first to show the plan.
5. The pin tools, each with the pins that are wired.
6. [bench_glasgow_safe_state](tools/bench_glasgow_safe_state) when done. Always, even if everything seemed to work.

Example: an I2C device on A0 (SCL) and A1 (SDA), 3.3 V:

```
bench_glasgow_port_set {"port": "A", "volts_v": 3.3, "confirm": true}
bench_glasgow_i2c_scan {}                      -> {"addresses": [80], ...}
bench_glasgow_i2c_read {"address": 80, "register_hex": "00", "count": 16}
bench_glasgow_safe_state {}
```

One session at a time: calls to one instrument are serialised by the server and an `flock` on `<install>/glasgow.lock` keeps a second process out (it waits two seconds, then reports who). Do not run `glasgow` by hand on the same device while the server is in use. Captures land in `BENCH_MCP_OUT`.
