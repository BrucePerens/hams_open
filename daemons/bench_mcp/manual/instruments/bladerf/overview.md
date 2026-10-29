---
title: bladeRF and XB-200 overview
summary: What the bladeRF 1 with XB-200 is, its range and what the SDR tools do with it (receive only).
see_also: [instruments/bladerf/setup, instruments/bladerf/operation, instruments/bladerf/limits]
tools: [bench_status, bench_identify, bench_sdr_capture, bench_sdr_survey]
instrument: bladerf
verified: code only, 2026-10-29
---

# bladeRF and XB-200 overview

A Nuand bladeRF 1 (115 kLE, USB 1d50:6066), firmware 2.6.0 with FPGA 0.16.0 flashed, plus an XB-200 transverter board. Driver `bladerf`, through SoapySDR (libSoapySDR is loaded at run time; without it the tools say so). Receive only: there is no transmit tool.

* Tuning 237.5 MHz to 3.8 GHz alone; with the XB-200 enabled (configuration `xb200`) 9 kHz to 3.8 GHz. Sample rate up to 20 MS/s, gain 0 to 70 dB in the tools.
* One receive channel. [bench_sdr_coherence](tools/bench_sdr_coherence) needs two and refuses it.
* Uses on the bench: an independent receiver to look at the VNA's output and modulation, noise surveys, a receiver for site studies.
* Levels are dBFS at the set gain, not calibrated power.
