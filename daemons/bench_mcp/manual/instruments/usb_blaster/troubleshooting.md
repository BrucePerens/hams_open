---
title: USB-Blaster troubleshooting
summary: Errors from the cable and the chain scan and what to do.
see_also: [instruments/usb_blaster/setup, tools/bench_jtag_scan]
instrument: usb_blaster
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# USB-Blaster troubleshooting

* `Access denied (insufficient permissions)` or no cable listed: the user cannot open the USB device; see [setup](instruments/usb_blaster/setup).
* `JTAG init failed with: TDO is stuck at 0` (seen on the dev box, 2026-11-23): the cable answers but nothing drives TDO. The target board is unpowered, the JTAG header is not connected, or the cable is plugged into the wrong header.
* `cannot start openFPGALoader`: install it or set `program`.
* A load fails half way: the SRAM design is undefined; power-cycle the board. A failed flash write may leave the flash half written: retry the same write or recover over JTAG with the original image; do not power-cycle before that.
