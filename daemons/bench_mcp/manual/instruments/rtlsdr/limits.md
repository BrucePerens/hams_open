---
title: RTL-SDR limits
summary: Ranges, the sample-rate gap, ppm, direct sampling, and what is not verified.
see_also: [instruments/rtlsdr/operation]
instrument: rtlsdr
verified: hardware, 2026-11-11
---

# RTL-SDR limits

* Receive only. There is no transmit path in the hardware or in the tools.
* Tuning 24 to 1766 MHz (R820T; the driver reports 24 to 1764 MHz and the part tunes some way past in practice, with a gap in the middle of the range on some units); with `direct_samp` 0.1 to 28.8 MHz instead (the tuner is bypassed, so there is no tuner gain or filtering).
* Sample rate 0.225 to 0.3 MS/s or 0.9 to 3.2 MS/s; nothing between 0.3 and 0.9. Above about 2.4 MS/s a computer's USB may drop samples (watch `continuous`). 8-bit samples: about 48 dB of dynamic range, so a strong signal makes images and clipping (`clipped_samples`).
* Gain 0 to 49.6 dB (the tuner's gain table); the tools do not use the automatic gain control.
* `ppm` -200 to 200, applied with the driver's frequency correction; the crystal also drifts as the dongle warms up for several minutes.
* 4096 to 16 777 216 samples per capture; one channel; no `antenna` argument.
* DC spike at the centre frequency and tuner images are the dongle's own.
* Not verified: absolute level, `ppm` against a reference, `direct_samp` on hardware, two dongles at once.
