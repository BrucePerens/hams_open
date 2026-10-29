---
title: bladeRF troubleshooting
summary: Symptom, cause and action for the bladeRF.
see_also: [instruments/bladerf/setup, concepts/errors]
instrument: bladerf
verified: code only, 2026-10-29
---

# bladeRF troubleshooting

| Symptom | Cause | Action |
|---|---|---|
| tool says libSoapySDR or its module is missing | not installed | install `libsoapysdr0.8` and `soapysdr-module-bladerf` |
| radio not found | not plugged in, USB power, or `serial` wrong | `bench_usb_topology`, check `serial` |
| tuning below 237.5 MHz refused | no `xb200` in the entry | add it to the configuration (owner) |
| nothing received, FPGA error | FPGA not loaded | load it with `bladeRF-cli -L hostedx115.rbf` (owner) |
| `continuous` false | samples lost | lower `sample_rate_hz` or `samples`; check USB 3 port |
| signal not heard on a LimeSDR-only option | `antenna` applies to the LimeSDR only | not applicable to the bladeRF |
