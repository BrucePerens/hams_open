---
title: ELAD FDM receivers: overview
summary: What the FDM-S2, FDM-DUO and FDM-S1 are and what the tools can read from them.
see_also: [instruments/elad/setup, instruments/elad/limits]
instrument: elad
verified: measured on the unit (FDM-S2, USB firmware 3.0, hardware 1.3; identity and calibration reads only), 2026-11-08
---

# ELAD FDM receivers: overview

ELAD S.r.l. makes direct-sampling HF receivers that deliver one wideband complex (I/Q) channel over USB 2.0. The ADC runs at 122.88 MHz (61.44 MHz in the FDM-S1) and a down-converter in an FPGA outputs 192 kHz to 6.144 MHz of baseband around a centre frequency you set.

| Model | USB id | Range | Notes |
|---|---|---|---|
| FDM-S2 | 1721:061c | 10 kHz to 54 MHz | receive only, bus powered; needs an FPGA image loaded before it streams; switchable band filters and attenuator |
| FDM-S1 | 1721:0610 | 10 kHz to 30 MHz | as the S2 without 6.144 MHz; not seen on this bench |
| FDM-DUO | 1721:061a | 10 kHz to 54 MHz | a transceiver; only its receive interface is used here; loads its own FPGA; its sample rate is set by the unit; not seen on this bench |

The tools open the unit, do their work and close it inside each call, so tuning, rate, filters and attenuator are arguments. The USB interface is vendor specific: control requests on endpoint 0 and bulk endpoints 0x02 (OUT) and 0x86 (IN, the samples).
