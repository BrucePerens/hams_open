---
title: ELAD FDM receivers: protocol notes
summary: The command subset and where each fact came from.
see_also: [instruments/elad/limits, concepts/extending/provenance]
instrument: elad
verified: measured on the unit (FDM-S2, USB firmware 3.0), 2026-11-08, for the rows marked observed; the rest fixtures only
---

# ELAD FDM receivers: protocol notes

Vendor control requests on endpoint 0 and one bulk IN endpoint, in the `elad_usb` crate (`daemons/elad_usb`, whose README holds the function list). Used: firmware version (`0xFF`), EEPROM read (`0xA2`), sampler tune (`0xF2`), front end (`0xF7`), FIFO start and stop (`0xE9`), and the FDM-DUO gateway (`0xE1`) with its tune, FIFO and status sub-commands. No request that transmits, writes CAT or writes the EEPROM exists in the crate, and the crate refuses to send one.

| Fact | Source kind | Source |
|---|---|---|
| USB ids, descriptors (interface 0, bulk 0x02 OUT and 0x86 IN), high speed | observation of the device | the owner's FDM-S2, 2026-11-08 |
| firmware `3.0`, hardware `1.3`, EEPROM map and values | observation of replies | the owner's FDM-S2, 2026-11-08 (fixture `tests/elad_fixtures/fdm_s2_fw3_0_open.txt`, serial replaced by `SN-0000`) |
| request codes, tuning-word formula, filter codes, sample widths | other public documentation | the public READMEs and issue text of two GPL-3.0 projects (SDRoxide, SoapyELAD): numbers and formulas only, no code |
| FPGA image requirement of the S1/S2 | observation and other public documentation | no samples without it on the owner's unit; the projects above |

Nothing in this module comes from vendor source, firmware, drivers or help text. ELAD's loader and its `gr-elad` source were not downloaded or read.
