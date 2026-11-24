---
title: USB-Blaster protocol
summary: The openFPGALoader command lines the driver runs and how it reads their output.
see_also: [instruments/usb_blaster/limits]
instrument: usb_blaster
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# USB-Blaster protocol

No raw JTAG is spoken by this server. The command lines (program `openFPGALoader`):

| Tool | Command |
|---|---|
| scan | `--scan-usb`, then `-c <cable> --detect` |
| sram_load | scan, then `-c <cable> -m <file>` |
| flash_write | scan, then `-c <cable> -f <file>` |

`--scan-usb` prints a table (bus, device, vid:pid, probe type, manufacturer, serial, product); the driver returns bus, device, vid:pid, probe and manufacturer and drops the serial number. `--detect` prints `index N:` blocks with `idcode`, `manufacturer`, `family`, `model` and `irlength`; the driver returns them as lists. A non-zero exit or an empty chain is an error carrying the program's last line.
