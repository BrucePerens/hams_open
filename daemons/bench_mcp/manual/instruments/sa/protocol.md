---
title: SSA3000X protocol notes
summary: The SCPI subset used, the quirks found on the unit, and where each fact came from.
see_also: [instruments/sa/limits, concepts/extending/provenance]
instrument: sa
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# SSA3000X protocol notes

SCPI over TCP port 5025, one command per line, replies end in a line feed. Used: `*IDN?`, `*OPC?`, `:SYSTem:OPTions?`, `:INITiate:CONTinuous`, `:INITiate:IMMediate`, `:UNIT:POWer`, `:DISPlay:WINDow:TRACe:Y:SPACing`, `:DISPlay:WINDow:TRACe:Y:RLEVel`, `:FORMat:TRACe:DATA ASCii`, `:SENSe:FREQuency:STARt|STOP`, `:SENSe:BWIDth:RESolution` and `:VIDeo` (with `:AUTO`), `:SENSe:POWer:RF:ATTenuation` (with `:AUTO`), `:SENSe:DETector:TRACe1:FUNCtion`, `:TRACe1:MODE`, `:SENSe:AVERage:TRACe1:COUNt|CLEar`, `:SENSe:SWEep:TIME|COUNt`, `:TRACe:DATA? 1`, `:CALCulate:MARKer<n>:STATe|MODE|TRACe|X|Y|MAXimum[:NEXT|:LEFT|:RIGHt]`, `:OUTPut:STATe`, `:SOURce:POWer`, `:SOURce:CORRection:OFFSet?`, `:HCOPy:SDUMp:DATA?`.

Found on the unit (SSA3021X, firmware 1.3.9.10, 2026-11-05):

* `:TRACe:DATA? 1` returns 751 comma-separated values in dBm as one line (ASCII is the default format).
* `*OPC?` after `:INITiate:IMMediate` answers only when the sweep, or all averaged sweeps, is done.
* Setting the reference level before the attenuator left the reference level at its old value; the driver sets the attenuator first and reads both back.
* `:SYSTem:ERRor?`, `:TRACe:SWEep:STATe?` and `:SENSe:SWEep:POINts?` get no reply; the driver does not use them.
* `:HCOPy:SDUMp:DATA?` returns a headerless 1024 x 600 24-bit BMP (1 843 254 bytes) and then a line feed.
* `:SYSTem:OPTions?` lists option names only (`Meas EMI TG CAT`), not whether they are permanent.
* Over USB the replies are cut short; only the LAN is used.

| Fact | Source kind | Source |
|---|---|---|
| command names, ranges, argument forms | vendor programming manual | Siglent SSA3000X Programming Guide PG0703X E03D, read for facts; nothing copied |
| the behaviour listed above | observation of the device's replies | the owner's unit, 2026-11-05 |
| `tests/sa_fixtures/status.txt` | capture of the owner's own session | scrubbed: serial `SN-0000`, no address |

Nothing was copied from vendor source, firmware, drivers or help text.
