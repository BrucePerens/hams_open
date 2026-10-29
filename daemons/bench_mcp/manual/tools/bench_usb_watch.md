---
title: bench_usb_watch
summary: Watch the USB tree for up to 120 s and report every change with its time.
see_also: [tools/bench_usb_history, instruments/usb-hubs/operation]
tools: [bench_usb_watch]
instrument: usb-hubs
tier: read_only
verified: idle watch on the dev box; a scripted power cycle in tests, 2026-10-29
---

# bench_usb_watch

Polls `/sys/bus/usb/devices` every `interval_ms` (default 200, 50 to 5000) for `seconds` (1 to 120). Reports devices appearing, disappearing and re-enumerating (new device number), speed and
authorized changes, port state changes and over-current count rises, each with its time since the start; and the enumerations per device and which devices flapped (3 or more).
Polling can miss a change shorter than the interval; the kernel journal (`bench_usb_history`) is exact. Start the watch, then act (for example a port cycle from another call) to see the sequence.

## Arguments

<!-- arguments -->
