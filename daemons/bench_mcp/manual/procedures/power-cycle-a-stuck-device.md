---
title: Power-cycle a stuck device
summary: The escalation ladder from a soft reset to port power for a USB device that stopped answering, including the VNA.
see_also: [concepts/errors, tools/bench_usb_diagnose, tools/bench_usb_port_power, tools/bench_vna_power_cycle]
tools: [bench_usb_diagnose, bench_usb_reset, bench_usb_port_power, bench_vna_power_cycle]
instrument: [usb-hubs, vna]
verified: VNA cycle through hub 3-2 port 2 confirmed 2026-10-29
---

# Power-cycle a stuck device

1. Confirm nothing else is using the device (a running sweep, a recording).
2. `bench_usb_diagnose {"device": "<path or label>"}` and read the findings; `bench_usb_protection` says whether the target may be touched.
3. Least disruptive first: `bench_usb_reset` (soft reset), then `bench_usb_authorize` (deauthorize and authorize), then `bench_usb_driver` (unbind and bind). Use `dry_run` first.
4. Power: `bench_usb_port_power {"action": "cycle", ...}` (tier full) with `expect_vidpid`. The ladder is described in the [USB hub operation page](instruments/usb-hubs/operation).
5. For the VNA use `bench_vna_power_cycle`, which waits for the unit and both codecs and verifies with a sweep; it also stops AM. The unit loses all settings.
6. Still failing after one cycle: stop, report the diagnose findings and ask the person.
