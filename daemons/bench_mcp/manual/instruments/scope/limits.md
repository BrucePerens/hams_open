---
title: Scope limits
summary: Ranges, accuracy, quirks and what is not wrapped or verified.
see_also: [instruments/scope/operation, instruments/scope/troubleshooting]
instrument: scope
verified: probed on the unit (SDS1202X-E, firmware 1.3.27), 2026-10-28
---

# Scope limits

* Samples are 8 bit: spectrum lines more than about 45 dB below the strongest are unreliable. Bandwidth 200 MHz; higher lines read low.
* Frequency counter wrong above about 150 MHz; use the spectrum for tones. Timebase accuracy about 3.5 ppm off on the tested unit.
* `PAVA? ALL` and the statistics query `PACU?` did not answer. Trigger types accepted: EDGE, SLEW, GLIT, INTV, RUNT, TV.
* After firmware 1.3.27 replies come without the `C1:VDIV ` prefix; the driver takes the value after the last space or comma.
* `*CAL?` answers 0 at once and the calibration keeps running.
* Waveform reads use `WFSU SP,1,NP,0,FP,0` (every point); without it the unit returned a few samples.
* Not wrapped: cursors, math, serial decode, the scope's own FFT. Only channel 1 is cabled here.
* Not verified: any behaviour on other SDS1000X-E firmware.
