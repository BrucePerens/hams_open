---
title: bench_rx888_status
summary: RX-888: on the USB bus, booted or not, link speed; the radiod this server started; the front end if radiod answers. Changes nothing.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_status]
instrument: rx888
tier: read_only
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_status

Reads `/sys/bus/usb/devices` for `04b4:00f3` (boot loader) and `04b4:00f1` (booted), checks the `radiod` this server started, reads the loopback multicast flag, and polls the front end for up to 0.8 s. Changes nothing.

## Arguments

<!-- arguments -->

## Returns

`usb` (`state`: `booted`, `unbooted (FX3 loader)...` or `not on the USB bus`; `devices` with `sysfs`, `usb_id`, `speed_mbps`, `superspeed`), `radiod` (`running`, `pid`, `config`, `log`, `uptime_s`), `front_end` (the fields of bench_rx888_frontend, or `answering: false` and why), `loopback_multicast`, `groups`.

## Failure modes

* `front_end.answering: false`: `radiod` is not running or uses other groups; see [troubleshooting](instruments/rx888/troubleshooting).

Example: `bench_rx888_status {}` returns `{"usb": {"state": "booted", "devices": [{"speed_mbps": 5000, ...}]}, "radiod": {"running": true, ...}, ...}`.
