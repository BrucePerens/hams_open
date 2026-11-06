---
title: bench_rx888_boot
summary: RX-888: loads the FX3 firmware into RAM with rx888_boot and waits for the booted device (04b4:00f1); reports its link speed.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_boot]
instrument: rx888
tier: read_config
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_boot

Runs ka9q-radio's `rx888_boot` with the configured absolute firmware path, then waits up to 20 s for `04b4:00f1`. The device leaves the USB bus and comes back; nothing is written to flash. Refused while this server runs `radiod`. If the device is already booted, says so and runs nothing.

## Arguments

<!-- arguments -->

## Returns

`booted`, `sysfs`, `speed_mbps`, `superspeed`, `note` (a non-USB 3 link limits the rate), `loader_output`; or `already_booted` with the USB state.

## Failure modes

* `no RX-888 on the USB bus`: check the cable, or power-cycle its hub port.
* `did not come back booted`: power-cycle its hub port (bench_usb_port_power off, 5 s, on) and call this once more; do not unbind the USB controller.
* `firmware ... is not a file`: set `firmware` in the config.

Example: `bench_rx888_boot {}` returned `{"booted": true, "speed_mbps": 5000, "superspeed": true, ...}`.
