---
title: bladeRF limits
summary: Ranges, quirks (overflow flags) and what is not verified.
see_also: [instruments/bladerf/operation]
instrument: bladerf
verified: code only, 2026-10-29
---

# bladeRF limits

* Tuning 237.5 MHz to 3.8 GHz, or 9 kHz to 3.8 GHz with `xb200`; sample rate 0.25 to 20 MS/s; gain 0 to 70 dB; captures 4096 to 16 777 216 samples.
* With an FPGA newer than libbladeRF knows, the driver reports an overrun about every 504 samples without losing any. The tools trust the timing (`stream_seconds` against `real_time_seconds`, `continuous`) and report the driver's count only.
* The FPGA is RAM-loaded unless flashed; after a power cycle without flash the radio does not work.
* The XB-200's filter bank selects by frequency band; levels step at band edges.
* dBFS only; no absolute power calibration. Not verified: absolute level accuracy, noise figure, the XB-200 switching on every setting.
