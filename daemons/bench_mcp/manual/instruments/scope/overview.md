---
title: Siglent SDS1202X-E overview
summary: What the oscilloscope is, the specifications that matter, its connectors and what the tools do with it.
see_also: [instruments/scope/setup, instruments/scope/operation, instruments/scope/limits]
tools: [bench_status, bench_measure, bench_capture, bench_scope_spectrum, bench_screenshot, bench_set, bench_scope_self_cal]
instrument: scope
tier: read_only
verified: probed on the unit (SDS1202X-E, firmware 1.3.27) via SCPI, 2026-10-28
---

# Siglent SDS1202X-E overview

A two-channel digital oscilloscope of the Siglent SDS1000X-E family, driver `siglent_sds`, written against an SDS1202X-E on firmware 1.3.27 (the README records the probing of 2026-10-28).

* Two analog inputs, 200 MHz bandwidth, 8-bit samples, 1 MOhm input (a 50 ohm source into it reads twice its loaded level). Memory depth up to 14 Mpoints (`14K`, `140K`, `1.4M`, `14M`).
* Controlled over the LAN with SCPI on TCP port 5025. USB is not used: the scope's USB descriptor declares 64-byte bulk packets at high speed, so only short replies work over USB (a waveform read stops after 52 bytes with the kernel `usbtmc` driver); Ethernet has no such limit.
* There is no raw-SCPI tool. The functions wrapped are: state ([bench_status](tools/bench_status)), automatic measurements ([bench_measure](tools/bench_measure)), waveform download ([bench_capture](tools/bench_capture)), a host-side spectrum ([bench_scope_spectrum](tools/bench_scope_spectrum)), screen image ([bench_screenshot](tools/bench_screenshot)), settings ([bench_set](tools/bench_set)) and self-calibration ([bench_scope_self_cal](tools/bench_scope_self_cal)).
* Not wrapped: cursors (the unit answers `CRMS?`), channel math, serial-bus decode (not found), the scope's own FFT (reachable but its axis was not decoded).

Only channel 1 is cabled on the bench this manual was written for; check with the person before relying on channel 2.
