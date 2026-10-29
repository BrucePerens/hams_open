---
title: Supply protocol notes
summary: The SCPI subset the driver uses.
see_also: [instruments/psu/limits]
instrument: psu
verified: code only, 2026-10-29
---

# Supply protocol notes

SCPI over the serial port, 115200 8N1, from the vendor's ATM SCPI programming manual v1.0.0. Used: `OUTP?`, `OUTP ON|OFF`, `VOLT?`, `VOLT <v>`, `CURR?`, `CURR <a>`, `VOLT:LIM?`, `CURR:LIM?`, `MEAS:VOLT?`, `MEAS:CURR?`, `MEAS:POW?`, `MEAS:ALL:INFO?`, `SYST:ERR?`. The port is opened exclusively with `flock`.
