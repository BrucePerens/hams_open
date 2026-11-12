---
title: RTL-SDR overview
summary: What the RTL-SDR dongle is, what the tools do with it, and that it is receive only.
see_also: [instruments/rtlsdr/setup, instruments/rtlsdr/operation, instruments/rtlsdr/limits]
tools: [bench_status, bench_identify, bench_sdr_capture, bench_sdr_survey]
instrument: rtlsdr
verified: hardware, 2026-11-11 (a Realtek RTL2838 with R820T on the dev box)
---

# RTL-SDR overview

A Realtek RTL2832U (USB 0bda:2838, sold as RTL2838 DVB-T) with a Rafael Micro R820T tuner. Driver `rtlsdr`, through SoapySDR (`soapysdr-module-rtlsdr`). One receive channel, 8-bit I/Q from the chip.

**It is receive only. The hardware has no transmitter and these tools have no transmit function; nothing here can key or radiate.**

* Tuning about 24 to 1766 MHz (the tools accept 24 to 1766 MHz), sample rate 0.225 to 0.3 or 0.9 to 3.2 MS/s, tuner gain 0 to 49.6 dB in steps of a few tenths of a dB.
* One input, no `antenna` choice, one channel: [bench_sdr_coherence](tools/bench_sdr_coherence) does not apply.
* `ppm` corrects the dongle's crystal error (these crystals are often off by tens of ppm); the config key `direct_samp` makes it tune 0.1 to 28.8 MHz without an upconverter (see [limits](instruments/rtlsdr/limits)).
* Uses: a cheap second receiver, VHF/UHF monitoring (NOAA weather radio at 162.400 to 162.550 MHz, FM broadcast), checking a transmitter's presence nearby. With only a small indoor antenna, expect strong local signals only.
