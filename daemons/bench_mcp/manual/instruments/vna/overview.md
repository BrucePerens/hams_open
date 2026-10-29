---
title: VNWA 3E overview
summary: What the DG8SAQ VNWA 3E is, the specifications that matter, its connectors, and which of its modes the tools offer.
see_also: [instruments/vna/setup, instruments/vna/operation, instruments/vna/limits, instruments/vna/protocol]
tools: [bench_vna_sweep, bench_vna_generator_on, bench_vna_meter, bench_vna_power_cycle]
instrument: vna
verified: measured on the unit (VNWA 3E, firmware v5.22), 2026-10-29
---

# VNWA 3E overview

The DG8SAQ VNWA 3E is a USB vector network analyser: USB id 20a0:4118 plus two PCM2900C audio codecs behind its internal hub. Driver `vnwa_open` (a Rust driver written from observed USB and audio traffic of the owner's own unit; no vendor code or firmware is used or flashed).

* Ports: TX (signal out, about -17 dBm at the top level, 0 dB), RX (S21 input) and the reflection port (S11). With TX cabled to RX the unit measures a through. The ports are SMA female.
* Frequency 0 to 1.3 GHz in the tools (the 3E's range; the driver refuses above 1.3 GHz). Two DDS chips (AD9859, AD9951), clocked from a 12 MHz TCXO times 3, times 10 for RF (360 MHz) and 9 for the LO (324 MHz). The reference is always internal; nothing may be connected to the external clock connector.
* S21 and S11 are measured together from the audio of the two codecs at an IF of 12 kHz, 6 kHz or 4 kHz; the audio is turned into a complex ratio of measured over reference channel.
* Bus powered: no other supply. On the bench it sits on port 2 of the power-switching hub 3-2; switching that port off and on is a verified cold reset (the unit keeps no settings, back about 9 s after power-on).

Modes of the hardware (per the vendor's help) and what the tools offer:

| Mode | Tool | State |
|---|---|---|
| S21 and S11 sweep | [bench_vna_sweep](tools/bench_vna_sweep) | measured, agrees with the vendor program to about 1 % |
| Open/short/load/through calibration | [bench_vna_calibrate_measure](tools/bench_vna_calibrate_measure), [bench_vna_calibrate_build](tools/bench_vna_calibrate_build) | tested on synthetic terms and a through on the unit; real open/short/load standards not yet measured |
| Signal generator, CW | [bench_vna_generator_on](tools/bench_vna_generator_on) | verified on a scope |
| Signal generator, AM | [bench_vna_generator_am](tools/bench_vna_generator_am) | one decoded setting only |
| Frequency meter | [bench_vna_meter](tools/bench_vna_meter) | precision verified, sign and absolute accuracy not |
| Time domain (impulse and step) | none; code exists in the driver | not exposed |
| Spectrum analyser | none | not decoded |

The unit and its codecs belong to the VNA driver: see [reserved devices](concepts/reserved-devices).
