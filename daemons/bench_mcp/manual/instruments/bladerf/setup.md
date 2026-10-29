---
title: bladeRF setup
summary: Software, FPGA and firmware, XB-200 wiring and the configuration entry.
see_also: [instruments/bladerf/overview, instruments/bladerf/troubleshooting, concepts/configuration]
instrument: bladerf
verified: code only, 2026-10-29
---

# bladeRF setup

Configuration: `"blade": {"driver": "bladerf", "tier": "read_config", "xb200": "auto1db"}`. Optional `serial` (hex digits) picks one board of several. `xb200` is one of `50M`, `144M`, `222M`, `auto`, `auto1db`, `auto3db`, `custom`; it switches on the XB-200 and its filter bank so the receiver tunes from 9 kHz. Without `xb200` the tools refuse frequencies below 237.5 MHz.

Software: SoapySDR with `soapysdr-module-bladerf` (apt: `bladerf`, `libbladerf2`, `libsoapysdr0.8`). The FPGA must be loaded: on this unit it is flashed (`bladeRF-cli -L hostedx115.rbf`) so it autoloads; it needs firmware 2.4 or later for FPGA 0.16.

XB-200 wiring: RXIF to the bladeRF RX J53, TXIF to TX J54, and the RXFILT-ANT and TXFILT-ANT jumpers fitted. The antenna or signal source connects to the XB-200 RX input for 9 kHz to 300 MHz; do not connect a transmitter. Nothing in this server transmits.
