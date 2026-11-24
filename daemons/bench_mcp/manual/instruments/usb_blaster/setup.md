---
title: USB-Blaster setup
summary: Installing openFPGALoader, USB permissions and the config entry.
see_also: [instruments/usb_blaster/overview, instruments/usb_blaster/troubleshooting]
instrument: usb_blaster
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# USB-Blaster setup

1. Install the program: `sudo apt install openfpgaloader` (Debian; 0.13.1 was used). Nothing is downloaded as a binary.
2. Let the user running the server open the cable: openFPGALoader ships udev rules (`/usr/lib/udev/rules.d/99-openfpgaloader.rules`); a user in the `plugdev` group may open it. Without them the program says `Access denied (insufficient permissions)`.
3. Config entry: `{"driver": "usb_blaster", "tier": "read_config"}`. Optional keys: `program` (path of openFPGALoader), `cable` (`usb-blaster` or `usb-blasterII`), `allow_flash` (default false). Use tier `full` for the SRAM load.
4. Power the target board and connect its JTAG header before a scan. A cable alone cannot see a chain.

On the dev box the cable is USB 09fb:6001 and is wired to a small FPGA prototyping board of unknown model.
