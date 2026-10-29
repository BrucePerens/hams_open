---
title: VNA limits
summary: Ranges, accuracy, quirks and what is not verified for the VNWA tools.
see_also: [instruments/vna/monitoring-and-recovery, instruments/vna/operation, tools/bench_vna_sweep]
instrument: vna
verified: measured on the unit (VNWA 3E, firmware v5.22) except where stated, 2026-10-29
---

# VNA limits

* Ranges: 0 to 1.3 GHz (above 0), 1 to 401 points, `if_hz` 12000, 6000 or 4000, `point_ms` 0.2 to 100, `rf_level` 0 to 16383, generator level 0 to -50 dB. Limits of every argument are in the argument tables.
* Sweep values are raw ratios; against the vendor program on the same cables S11 agrees to 0.25 % and 0.6 degrees, S21 to 1 % (a constant 0.8 % offset) and 0.6 degrees.
* A calibration is valid only at the IF, point time, RF level and cable text of its standards and inside its range. The ratio depends on the drive level (cause not found).
* Calibration kits: open, short and load standards and the demo kit have not been measured on the unit; the kit-definition loss formula follows the usual convention, not checked against a real kit file. Time-domain transforms exist in the driver but have no tool; gating is not implemented.
* Generator: absolute level about -17 dBm at 0 dB into 50 ohms, flat to a few percent; steps 0 to -40 dB verified within 0.3 dB at 10 MHz; -50 dB not verified; level falls with frequency (not characterised here).
* AM: only 500 Hz at 100 % at -20 dB; FM, pulse and other settings are not decoded. The unit stalls control requests while modulating; recovery is a power cycle (after one cycle at 3 s off the control chip failed to enumerate; the driver escalates to a 12 s off time).
* Frequency meter: valid within +/-25 Hz of the centre; sign (higher RX frequency = positive offset) and absolute accuracy against a standard are not verified.
* Spectrum analyser mode: not decoded; use `bench_scope_spectrum` or an SDR.
* Low frequency: usable down to 100 Hz with lowered reference amplitude, refused at 70 Hz and below; the unit's response falls below 2 kHz (AC coupled).
* A generator command during a sweep or meter burst is overwritten; wait about 3 s.
* About 0.6 % of attempts fail at the first try and are cured by re-measurement; see the [monitoring page](instruments/vna/monitoring-and-recovery).
