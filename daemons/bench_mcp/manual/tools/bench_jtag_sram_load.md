---
title: bench_jtag_sram_load
summary: USB-Blaster: load a .sof or .svf into FPGA SRAM only; volatile.
see_also: [instruments/usb_blaster/overview, instruments/usb_blaster/limits]
tools: [bench_jtag_sram_load]
instrument: usb_blaster
tier: full
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# bench_jtag_sram_load

Loads a bitstream into the FPGA's SRAM. The running design is replaced until the next power cycle; the flash is never touched. `instrument` names the entry; `file` is the absolute path; `confirm_sram_volatile` must be true for a real load; `dry_run` shows the command and sends nothing.

## Arguments

<!-- arguments -->

## Returns

`loaded` (`sram`), `file`, `devices` (count on the chain), `output_tail` (last lines of the program's output) and a note that it is volatile.

## Failure modes

* `confirm_sram_volatile`: missing, nothing was sent.
* `no device answered on the JTAG chain`: nothing was loaded.
* `the SRAM load failed`: the design is undefined; power-cycle the board.

## Example

`bench_jtag_sram_load {"file": "/home/me/blink.sof", "dry_run": true}`
