---
title: Limits of the USB hub tools
summary: What the tools cannot know or do: VBUS, ganged hubs, USB 3 hubs, unlisted hubs.
see_also: [instruments/usb-hubs/operation, instruments/usb-hubs/troubleshooting]
tools: [bench_usb_hubs, bench_usb_port_power]
instrument: usb-hubs
tier: read_only
verified: hub 3-2 and hub 2-3 on the dev box, 2026-10-29
---

# Limits of the USB hub tools

* Software cannot see VBUS. The hub descriptor says what the hub claims; uhubctl says what it will try; a cycle shows the device left and returned. Only a meter on VBUS shows power fell.
* Only hubs uhubctl lists as per-port (`ppps`) are switched. A ganged hub, or a hub uhubctl does not list (root hubs here), is refused, because switching one port would not affect only that port. On this machine uhubctl lists hub 3-2 (the VNWA's hub) and the USB 3 hub 2-3; the VNWA's own internal hub 3-2.2 is ganged.
* A SuperSpeed hub is addressed with `uhubctl -e` (uhubctl 2.6.0 answers "No compatible devices detected at location 2-3" for `-l 2-3` without it); the tool adds it by itself.
* The kernel journal (history) is readable only by root or the groups adm and systemd-journal.
* The device speed finding uses `bcdUSB`; it cannot know whether a device that reports 2.x is high-speed capable.
