---
title: bench_usb_diagnose
summary: Explain why a USB device is slow, misbehaving or missing, with ranked findings and next steps.
see_also: [bench_usb_hubs, bench_usb_history, bench_usb_bandwidth, usb-hubs/troubleshooting]
tools: [bench_usb_diagnose]
instrument: usb-hubs
tier: read
verified: run on the dev box (VNWA codec, an empty port); the rules are tested against fixtures, 2026-10-29
---

# bench_usb_diagnose

Arguments: `device` (path or port label; a path or label with nothing on it is diagnosed as a missing device) or `hub` and `port`; `privileged: false` skips `sudo -n uhubctl`.

Checks: the hub is in the tree and authorized; for a missing device the port's status word (powered off, over-current, powered with nothing connected, connected but not enabled), the port's disable flag
and over-current count, and the kernel's enumeration failures; for a present device the speed against its descriptor, authorized, interfaces without a driver (a warning for audio, HID,
communications, storage and similar classes, a note for vendor-specific ones), runtime suspend, the power budget (bMaxPower against the hub's supply), translator sharing, and the last 24 h of the kernel journal
(-71 errors, enumeration failures, repeated resets, flapping). `could_not_read` lists what needed a privilege it did not have (the kernel journal needs the journal group).
