---
title: USB hub tools
summary: What the bench_usb_* tools describe and control, the tiers, and why they live in bench_mcp.
see_also: [instruments/usb-hubs/setup, instruments/usb-hubs/operation, instruments/usb-hubs/limits, instruments/usb-hubs/troubleshooting]
tools: [bench_usb_topology, bench_usb_find, bench_usb_hubs, bench_usb_protection, bench_usb_port_power]
instrument: usb-hubs
tier: read_only
verified: read-only tools run against the dev box tree (hub 3-2 and the VNWA chain), 2026-10-29
---

# USB hub tools

The `bench_usb_*` tools describe and control the host's USB tree: which device is where, at what negotiated speed, on which port of which hub, which `/dev` node it exposes, what
each hub claims it can switch, and, where a hub really switches port power, off, on and cycle for one port. They are in bench_mcp (`src/usb_hub/`) because the VNA is attached through a
power-switching hub: the VNA cold reset and `bench_usb_port_power` are one implementation of power off, power on and the wait for re-enumeration.

* The information tools read `/sys/bus/usb/devices` and need no privilege. `bench_usb_hubs` also runs `sudo -n uhubctl` and `sudo -n lsusb -v`, because the hub class descriptor and the raw port status words are readable only by root.
* Port power uses `sudo -n uhubctl` only.
* The instrument entry's tier decides what is listed: `read_only` the information tools, `read_config` also switching a port on, `full` also switching off and cycling.
