---
title: NanoVNA-H as a second VNA driver
summary: The NanoVNA family (ChibiOS USB serial shell) behind the same bench_vna tools: what it does, what it refuses, how it is configured, and what is and is not verified on hardware.
see_also: [instruments/vna, instruments/vna/limits, tools/bench_vna_sweep, tools/bench_vna_calibrate_measure, concepts/errors]
tools: [bench_vna_sweep, bench_vna_calibrate_measure, bench_vna_calibrate_build, bench_vna_power_cycle, bench_identify, bench_status]
instrument: vna
verified: not verified on hardware. The unit on the dev box (0483:5740, /dev/serial/by-id/usb-STMicroelectronics_ChibiOS_RT_Virtual_COM_Port_400-if00) did not answer any shell command on 2026-10-31. Parser and driver tested against an in-memory fake, a pty fake and a synthetic transcript only.
---

# NanoVNA-H as a second VNA driver

Set `"driver": "nanovna"` in the bench config to put a NanoVNA behind the `bench_vna_*` tools. The default driver stays `vnwa_open`.

    "nano": {"driver": "nanovna", "tier": "read_config",
             "serial": "/dev/serial/by-id/usb-STMicroelectronics_ChibiOS_RT_Virtual_COM_Port_400-if00"}

Optional keys: `min_hz`, `max_hz`, `max_points` (override the limits taken from the board name in `info`), `settle_ms` (wait per point after the range is set, default 12), `use_scan` (default true). `hub` and `hub_port` enable the power-cycle tool.

## What works

* `bench_identify`, `bench_status`: `version`, `info` and `help` are read; the answer gives the version, board, command list, whether `scan` exists and the sweep limits.
* `bench_vna_sweep`: `start_hz`, `stop_hz`, `points`, `calibration`, `cable`. The CSV and the JSON rows are the same as the VNWA's (S21 and S11 as real and imaginary parts, dB and degrees). Limits come from the firmware's board: 50 kHz to 900 MHz and 101 points for the NanoVNA-H, more for the H4 (401 points). At least 2 points and `stop_hz` above `start_hz`. `if_hz`, `point_ms` and `rf_level` are VNWA settings and are refused as not supported.
* With `scan` in the firmware's `help`, one `scan start stop points 7` returns frequency, S11 and S21. Without it (or with `use_scan` false) the driver reads the sweep range, sets the new one, `resume`, waits, `pause`, reads `frequencies`, `data 0` (S11) and `data 1` (S21), then restores the range and `resume`s. The unit's range is the only thing changed, in RAM.
* `bench_vna_calibrate_measure` and `_build` use the same open/short/load/thru arithmetic as the VNWA, in software on the points read; the settings line stored with each standard has no IF, time or level, only the cable text. A calibration is refused when the cable text differs. The NanoVNA's own calibration slots are never written.
* `bench_vna_power_cycle` works when `hub` and `hub_port` are configured; otherwise it is not supported.

## What is refused

`bench_vna_generator_on`, `_off`, `_level`, `_am`, `_fm`, `bench_vna_meter` and `bench_vna_spectrum` return `not supported by this instrument (nanovna): ...`. A missing or unplugged unit returns `device not present: ...` from every tool; a unit that is plugged in but silent returns `nanovna timeout: no prompt ...`.

## Caveats

* The values are what the unit reports with its own calibration if one is active (`data 0` and `data 1` are corrected by the unit); this driver cannot tell. Treat uncalibrated work as the unit's choice, or clear it at the unit.
* The unit sweeps by itself; the driver waits `300 ms + settle_ms * points` after setting the range, which is not checked against a sweep-complete flag.
* Not hardware-verified: everything above. The protocol facts come from public NanoVNA documentation, and the test transcript in `tests/nanovna_fixtures` is synthetic. The unit on the dev box did not answer at all (writes to its serial port were not accepted, no byte was ever received) and then dropped off the USB bus after a USB deauthorize and driver rebind attempted from the host; it needs to be unplugged and replugged, then these facts re-checked with `bench_identify`.
