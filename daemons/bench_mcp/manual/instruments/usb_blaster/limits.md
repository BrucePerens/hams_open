---
title: USB-Blaster limits
summary: What the driver refuses and why.
see_also: [instruments/usb_blaster/operation, concepts/safety-tiers]
instrument: usb_blaster
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# USB-Blaster limits

* SRAM load: only `.sof` and `.svf`, absolute path, an existing regular file of 1 byte to 128 MiB. Always the `-m` (SRAM) option, never `-f`.
* Flash write: refused unless the config entry has `allow_flash: true`; refused without `owner_approval` (the owner's words for this one write) and `confirm_flash` equal to `WRITE FLASH <file name>`. Only `.jic`, `.rpd`, `.pof`, `.rbf`, `.bin`. Not enabled by default, ever; no agent turns it on.
* The cable can drive the board's pins: a loaded design may drive outputs. Check the board before an SRAM load.
* The driver does not read or save flash, change fuses, or touch any other device. It does not transmit anything.
