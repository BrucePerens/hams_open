---
title: ELAD FDM receivers: operation
summary: Step-by-step use of the five tools.
see_also: [instruments/elad/setup, tools/bench_elad_capture]
instrument: elad
verified: fixtures only
---

# ELAD FDM receivers: operation

1. [bench_elad_list](tools/bench_elad_list): is a receiver on the bus, which model, and does it stream under this config.
2. [bench_elad_identify](tools/bench_elad_identify): firmware, hardware version, serial and the EEPROM calibration (rate correction in Hz; global, low-pass and attenuator offsets in dB).
3. [bench_elad_spectrum](tools/bench_elad_spectrum): a CSV of the spectrum around `freq_hz` and the strongest peaks. Start at 192 kHz.
4. [bench_elad_level](tools/bench_elad_level): the level at a frequency in dBFS, with the calibration offset.
5. [bench_elad_capture](tools/bench_elad_capture): a cf32 file. Check `measured_rate_hz` against `rate_hz` and `bulk_timeouts` before trusting a recording: the stream has no sequence numbers, so loss shows as a low rate or timeouts.

Front end (FDM-S2): `filters` (default in) selects the band filter for the frequency; `attenuator` adds the attenuator. The FDM-DUO and FDM-S1 have no such switches here and refuse them.
