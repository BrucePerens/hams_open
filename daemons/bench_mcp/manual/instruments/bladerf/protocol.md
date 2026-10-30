---
title: bladeRF and XB-200 protocol notes
summary: The SoapySDR calls the receive tools make and where each fact came from.
see_also: [instruments/bladerf/limits, instruments/bladerf/operation, concepts/extending/provenance]
instrument: bladerf
verified: code only, 2026-10-29
---

# bladeRF and XB-200 protocol notes

There is no radio protocol in the server. The radio is driven through SoapySDR, whose shared library is loaded at run time (`dlopen`), so the server runs without it and the SDR tools then say that SoapySDR is not installed. Only receive is implemented; nothing here transmits.

Calls used, in order, for one capture: `SoapySDRDevice_makeStrArgs` (the driver name and, if configured, the hexadecimal `serial`), `getDriverKey`, `getHardwareKey` and `getHardwareInfo` for the identification, `setSampleRate`, `setFrequency`, `setGain`, `setAntenna` (when one is asked for), `setupStream` and `activateStream` for complex float samples, repeated `readStream`, then `deactivateStream`, `closeStream` and `unmake`. Errors are read with `SoapySDRDevice_lastError`. The bladeRF has one receive channel. The XB-200 is configured with the `xb200` key (`50M`, `144M`, `222M`, `auto`, `auto1db`, `auto3db` or `custom`), passed to SoapySDR when the device is opened; below 237.5 MHz it is required.

The analysis (spectrum, peaks, DC removal, noise floor) is done on the host in `src/dsp.rs`; levels are dBFS of the samples at the set gain.

| Fact | Source kind | Source |
|---|---|---|
| the function names and argument types | other public documentation | the SoapySDR C API header (`Device.h`) |
| the radio's own behaviour (overruns, firmware, FPGA loading) | observation of the device's replies | runs on the dev box, recorded in [limits](instruments/bladerf/limits) |

Nothing was copied from vendor source, firmware, drivers or help text.
