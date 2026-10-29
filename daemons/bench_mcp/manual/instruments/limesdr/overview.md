---
title: LimeSDR-USB overview
summary: What the LimeSDR-USB is, its two coherent receivers and what the tools do with it (receive only).
see_also: [instruments/limesdr/setup, instruments/limesdr/operation, instruments/limesdr/limits]
tools: [bench_status, bench_identify, bench_sdr_capture, bench_sdr_survey, bench_sdr_coherence]
instrument: limesdr
verified: code only, 2026-10-29
---

# LimeSDR-USB overview

A LimeSDR-USB (USB 1d50:6108, hardware v4), LMS7002M: 2 receive and 2 transmit channels, one shared local oscillator so the two receivers are phase-coherent. Driver `limesdr`, through SoapySDR (`soapysdr-module-lms7`, LimeSuite 23.11). Firmware 4 and gateware 2.23. Receive only here: no transmit tool.

* Tuning 30 MHz to 3.8 GHz in the tools, sample rate up to 30.72 MS/s, 12-bit.
* Receive inputs per channel: `LNAH`, `LNAL`, `LNAW` (the H, L and W connectors). The tools default to `LNAL`.
* Uses: a phase-coherent receiver pair (for example direction finding), a second independent receiver. [bench_sdr_coherence](tools/bench_sdr_coherence) measures phase and gain between the channels given one signal split to both.
