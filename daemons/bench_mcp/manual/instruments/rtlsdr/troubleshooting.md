---
title: RTL-SDR troubleshooting
summary: Symptom, cause and action for the RTL-SDR.
see_also: [instruments/rtlsdr/setup, concepts/errors]
instrument: rtlsdr
verified: hardware, 2026-11-11
---

# RTL-SDR troubleshooting

| Symptom | Cause | Action |
|---|---|---|
| the tool says SoapySDR is not installed, or no rtlsdr device | `libsoapysdr0.8` or `soapysdr-module-rtlsdr` missing | install them |
| cannot open: usb_claim_interface error | the kernel DVB driver or another program (`rtl_sdr`, SDR++) has the dongle | blacklist the DVB modules, unplug and replug, close the other program |
| cannot open: permission denied | no udev rule for the user | install the `rtl-sdr` udev rules and replug |
| a sample rate between 0.3 and 0.9 MS/s is refused | the RTL2832U cannot run it | use 0.3 or lower, or 0.9 or higher |
| `continuous` false on a long capture | USB drops at a high rate | lower `sample_rate_hz` (2.048 MS/s is usually safe) |
| a station is found at the wrong frequency | crystal error | pass `ppm` |
| nothing is heard | small antenna or wrong band | try NOAA weather radio or the FM band first; check the antenna |
| `antenna must ...`, `channel` or coherence refused | one input and one channel | leave them out |
