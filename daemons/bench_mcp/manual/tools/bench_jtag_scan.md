---
title: bench_jtag_scan
summary: USB-Blaster: cables on the USB bus and the JTAG chain with IDCODEs, family and model.
see_also: [instruments/usb_blaster/overview, instruments/usb_blaster/limits]
tools: [bench_jtag_scan]
instrument: usb_blaster
tier: read_config
verified: cable detected on the dev box 2026-11-23 (chain scan failed: TDO stuck at 0, board not powered or not wired); everything else fixtures only
---

# bench_jtag_scan

Lists the USB-Blaster cables and scans the JTAG chain of the configured cable. Reads identification registers only. `instrument` names the entry when several are configured.

## Arguments

<!-- arguments -->

## Returns

`cables` (bus, device, vid_pid, probe, manufacturer; the serial number is not returned), `cable`, and `devices`, each with `idcode`, `manufacturer`, `family`, `model`, `irlength`.

## Failure modes

* `no USB-Blaster cable is on the USB bus`: not plugged in or no permission.
* `no device answered on the JTAG chain (... TDO is stuck at 0)`: board unpowered or not wired.

## Example

`bench_jtag_scan {}`
