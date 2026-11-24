---
title: bench_jtag_flash_write
summary: USB-Blaster: write the configuration flash; off by default, the owner approves every call.
see_also: [instruments/usb_blaster/overview, instruments/usb_blaster/limits]
tools: [bench_jtag_flash_write]
instrument: usb_blaster
tier: full
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# bench_jtag_flash_write

Writes a configuration flash image. Listed only when the config entry has `allow_flash: true`. Ask the owner for a yes for this write each time. `instrument` names the entry; `file` is the absolute path; `owner_approval` quotes the owner's yes; `confirm_flash` is exactly `WRITE FLASH <file name>`; `dry_run` shows the command and sends nothing.

## Arguments

<!-- arguments -->

## Returns

`written` (`flash`), `file`, `output_tail`.

## Failure modes

* `flash writes are off`: the config does not allow them; the owner decides that.
* missing `owner_approval` or wrong `confirm_flash`: nothing was sent.
* `the flash write FAILED`: the flash may be half written; do not power-cycle; retry or recover over JTAG.

## Example

`bench_jtag_flash_write {"file": "/home/me/board.jic", "dry_run": true}`
