---
title: USB-Blaster operation
summary: The order to use the tools in and what each returns.
see_also: [instruments/usb_blaster/limits, tools/bench_jtag_scan]
tools: [bench_jtag_scan, bench_jtag_sram_load, bench_jtag_flash_write]
instrument: usb_blaster
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# USB-Blaster operation

1. `bench_jtag_scan` first. It must list the device you expect.
2. `bench_jtag_sram_load` with `dry_run: true` to see the command, then with `confirm_sram_volatile: true`. The tool scans the chain again and refuses when nothing answers.
3. `bench_jtag_flash_write` only when the owner has turned on `allow_flash` in the config and says yes to this write. Read the board's original flash out first by another route so a bad image can be undone.

Every call is one program run with a time limit (20 s for a scan, 600 s for a load or write).
