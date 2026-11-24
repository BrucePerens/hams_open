---
title: USB-Blaster overview
summary: What the driver does with the USB-Blaster cable (USB 09fb:6001), its three tools and which tier each needs.
see_also: [instruments/usb_blaster/setup, instruments/usb_blaster/limits, concepts/safety-tiers]
tools: [bench_jtag_scan, bench_jtag_sram_load, bench_jtag_flash_write]
instrument: usb_blaster
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# USB-Blaster overview

The Altera USB-Blaster is a USB to JTAG cable. This driver runs the `openFPGALoader` program (Debian package `openfpgaloader`, version 0.13.1 tested against) with the cable `usb-blaster` or `usb-blasterII`. It is for looking at what is on a board's JTAG chain and for loading designs into FPGA boards on the bench. The cable carries no state; every call starts the program and ends it.

| Tool | Tier | Does |
|---|---|---|
| [bench_jtag_scan](tools/bench_jtag_scan) | read_config | cables on the USB bus; devices on the JTAG chain with IDCODE, family, model |
| [bench_jtag_sram_load](tools/bench_jtag_sram_load) | full | `.sof` or `.svf` into FPGA SRAM only (volatile) |
| [bench_jtag_flash_write](tools/bench_jtag_flash_write) | full, and `allow_flash` | configuration flash; owner's yes for every call |

The scan reads identification registers only. The SRAM load replaces the running design until the next power cycle and never touches the flash. The flash write is never listed unless the config entry has `"allow_flash": true`, and even then each call needs the owner's own yes for that one write.
