---
title: LimeSDR limits
summary: Ranges, antenna selection and what is not verified.
see_also: [instruments/limesdr/operation]
instrument: limesdr
verified: code only, 2026-10-29
---

# LimeSDR limits

* Tuning 30 MHz to 3.8 GHz; sample rate 0.25 to 30.72 MS/s; gain 0 to 70 dB; 4096 to 16 777 216 samples.
* The default receive input is `LNAL` at every frequency, whatever the band; a different connector must be chosen with `antenna`.
* Levels are dBFS at the set gain, not power. The DC spike at the centre is the radio's own.
* The antenna selection was unit tested; hardware testing of `antenna` on all three inputs is not recorded.
* Not verified: absolute level, phase of the two channels over temperature.
