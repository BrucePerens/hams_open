---
title: bench_status
summary: What an instrument is doing now: scope timebase and channels, supply output and readbacks, VNA link and settings, SDR identity.
see_also: [tools/bench_identify, tools/bench_set, instruments/scope, instruments/psu, instruments/vna/monitoring-and-recovery]
tools: [bench_status]
instrument: [scope, psu, vna, bladerf, limesdr]
tier: read_only
verified: code only, 2026-10-29
---

# bench_status

Reads the current state. Use it before changing anything and after any error from a settings tool.

## Arguments

<!-- arguments -->

* `instrument`: the alias; optional when only one instrument fits.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

## Returns

* **Scope**: `trigger_mode` (`AUTO`, `NORM`, `SINGLE`, `STOP`), `acquisition` (`running` or `stopped`), `timebase_s_per_div`, `sample_rate_sa_s`, and for channels 1 and 2 `vdiv_v`, `offset_v`, `coupling` (`A1M`, `D1M`, `GND`) and `attenuation` (probe ratio).
* **Supply**: `output` (`on`/`off`), `set_voltage_v`, `set_current_a`, `ovp_v`, `ocp_a` (the supply's own protection limits), `measured_voltage_v`, `measured_current_a`, and `error` (the last entry of the supply's error queue).
* **VNA**: `usb` (`present`, `present but not answering`, `not found or no permission`), the result of a harmless read, `instrument_monitoring` (firmware version and `temperature_counts`, see the [monitoring page](instruments/vna/monitoring-and-recovery)), `hub`, `power_cycles_by_this_server`, the codec ids, and the sweep defaults `if_hz`, `point_ms`, `rf_level`.
* **SDR**: the same as [bench_identify](tools/bench_identify).

## Failure modes

An unreachable scope or supply gives a connection or timeout error. A VNA that is listed on the bus but does not answer reports `present but not answering` (a power cycle is the cure, see [errors](concepts/errors)).

## Example

`bench_status {"instrument": "psu"}` answers with the fields above; with the output off the measured voltage is about 0 V and current 0 A.
