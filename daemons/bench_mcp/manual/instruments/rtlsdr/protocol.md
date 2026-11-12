---
title: RTL-SDR protocol notes
summary: The SoapySDR calls the receive tools make for the RTL-SDR and where each fact came from.
see_also: [instruments/rtlsdr/limits, instruments/rtlsdr/operation, concepts/extending/provenance]
instrument: rtlsdr
verified: hardware, 2026-11-11
---

# RTL-SDR protocol notes

There is no radio protocol in the server. The dongle is driven through SoapySDR, whose shared library is loaded at run time (`dlopen`); the same calls as for the other SDRs are used (see [LimeSDR protocol](instruments/limesdr/protocol)): open with `driver=rtlsdr` plus the configured `serial` and `direct_samp`, `setSampleRate`, `setGain`, `setFrequencyCorrection` (only when `ppm` is given), `setFrequency`, a CF32 stream, `readStream`. Only receive is implemented and nothing transmits.

| Fact | Source kind | Source |
|---|---|---|
| ranges, gain range, sample rates | observation of the device's replies | `SoapySDRUtil --probe='driver=rtlsdr'` on the dev box, 2026-11-11 |
| `direct_samp` device argument | observation of the device's replies | the same probe lists the setting (0, 1, 2) |
| the 0.3 to 0.9 MS/s gap | other public documentation | the RTL-SDR project's description of the RTL2832U |

Nothing was copied from vendor source, firmware, drivers or help text.
