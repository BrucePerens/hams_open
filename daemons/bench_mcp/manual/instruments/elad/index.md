---
title: ELAD FDM receivers
summary: ELAD FDM-S2, FDM-DUO and FDM-S1 USB receivers through the elad_usb crate: identify, spectrum, level and I/Q captures. Receive only.
see_also: [instruments/elad/overview, instruments/elad/setup, concepts/safety-tiers]
instrument: elad
verified: n/a (navigation)
---

# ELAD FDM receivers

The `bench_elad_*` tools drive an ELAD FDM-S2, FDM-DUO or FDM-S1 receiver directly over USB (no vendor library, no `radiod`): list the receivers, read the unit's identity and calibration, take a spectrum, measure a level and record I/Q. Nothing here transmits, and no tool needs a tier above `read_config`. An FDM-S1/S2 needs ELAD's FPGA image loaded before it streams; read the setup page first.

<!-- children -->
