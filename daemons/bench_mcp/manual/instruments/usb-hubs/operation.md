---
title: Using the USB hub tools
summary: Finding a device, reading hub capabilities, switching a port, the verification levels and the protection rules.
see_also: [instruments/usb-hubs/setup, instruments/usb-hubs/limits, instruments/usb-hubs/troubleshooting]
tools: [bench_usb_topology, bench_usb_find, bench_usb_hubs, bench_usb_protection, bench_usb_port_power]
instrument: usb-hubs
tier: full
verified: read tools on the dev box; off and on through the tool on an empty port of hub 2-3 with the real uhubctl; cycle with a device only against a scripted hub (2026-10-29)
---

# Using the USB hub tools

**Which device is this?** `bench_usb_find` with `node` (`ttyUSB0`, `hidraw3`, a sound card id such as `CODEC`, an interface name or a block name), `vidpid`, `product` or `label`. `bench_usb_topology` shows the tree, with the negotiated speed of each device and a finding when it is below what the device can do (a USB 3 device below SuperSpeed is a warning; a device that reports USB 2.x at 12 Mb/s is a note only, since many full-speed devices report 2.0).

**What can a hub do?** `bench_usb_hubs` gives, per hub, what uhubctl says about switching (per-port, ganged, or not listed), the hub's own descriptor (as it claims), the TT arrangement, and for each port its status bits (powered, connected, enabled, suspended, over-current, reset, speed, USB 3 link state) and its power-control verification level.

**Who shares what?** `bench_usb_bandwidth`: the devices behind one transaction translator and the periodic bandwidth they could reserve. **Why is it slow or missing?** `bench_usb_diagnose`.

**What happened to it?** `bench_usb_history` (journal: timeline, error counters, flapping) and `bench_usb_watch` (live). A device whose connects repeat within a short window is flapping; `-71` errors on enumeration point at the link (cable, hub, power).

**Verification levels** (a port cannot be verified from software alone):

1. claimed: uhubctl reports per-port switching.
2. re-enumerated: the device left the USB tree when the port went off and came back with a new device number and readable descriptors, with the measured times. Every cycle the tool performs does this check and reports it. It shows the port dropped the device, not that VBUS fell: some hubs disable the data lines and leave VBUS on. A level-2 result entered by hand (`confirmed_by_hand`, for example the VNWA on hub 3-2 port 2 goes dark and keeps no settings) is labelled "plus manually confirmed".
3. state cleared: a device-specific check that the device lost its settings; recorded in the config (`state_cleared`), not measured by the tool.
4. VBUS measured: an inline meter, manual entry only (`vbus_measured`).

**Switching a port.** `bench_usb_port_power` with `action` off, on or cycle, the port named by `hub` and `port`, `label` or `device`. Always run `dry_run: true` first: it lists the commands, the devices affected and the protection verdict, and changes nothing. A cycle keeps the port off for `off_time_s` (default 5 s, 1 to 60) and reports the actual off time, how long the device took to leave and to return, and the new device number.

**Escalation ladder** (each its own tool at the read_config tier, mildest first): `bench_usb_reset` (soft reset, keeps power and device number), `bench_usb_authorize` (deauthorize: software disconnect that keeps the power; authorize again), `bench_usb_driver` (unbind or bind one interface), `bench_usb_autosuspend` (on or auto), then `bench_usb_port_power` (full tier). All take `dry_run`.

**Protection.** Refused by default: the device carrying the root filesystem or another mounted USB filesystem, boot keyboards and mice, config-critical ports and devices, a bench instrument's serial adapter (not overridable), the VNA's hub port and everything below it and the VNA's own USB id (overridable, because a reset spoils a measurement), every hub on the path to those, and the uplink of any hub (unless `hub_uplink_ok`). `bench_usb_protection` lists them. `override_protection: true` overrides only the reasons that are not from the config or a bench instrument.
