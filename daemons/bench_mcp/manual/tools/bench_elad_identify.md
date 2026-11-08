---
title: bench_elad_identify
summary: ELAD receiver: firmware, hardware version, serial and EEPROM calibration by read-only requests. Streams nothing.
see_also: [instruments/elad/overview, instruments/elad/limits]
tools: [bench_elad_identify]
instrument: elad
tier: read_only
verified: measured on the unit (FDM-S2, USB firmware 3.0), 2026-11-08
---

# bench_elad_identify

Opens the first receiver and sends only the firmware-version and EEPROM read requests. It starts no stream and tunes nothing.

## Arguments

<!-- arguments -->

## Returns

`model`, `usb_id`, `firmware`, `hardware`, `serial`, `rate_correction_hz`, `offset_global_db`, `offset_lp_db`, `offset_att_db` (a field the unit never had programmed is null), `rx_range_hz`, `sample_rates_hz`, `needs_fpga_image`, `streams_under_this_config`.

## Failure modes

`could not open the receiver (Access denied)`: see [setup](instruments/elad/setup).

Example: `bench_elad_identify {}` on the FDM-S2 returned `{"model": "FDM-S2", "firmware": "3.0", "hardware": "1.3", "rate_correction_hz": -784, "offset_global_db": -8.0, "offset_att_db": 12.5, "needs_fpga_image": true, ...}`.
