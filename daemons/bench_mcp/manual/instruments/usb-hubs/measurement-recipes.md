---
title: USB measurement recipes
summary: Finding a device that flaps on the bus, step by step with the USB tools.
see_also: [tools/bench_usb_history, tools/bench_usb_watch, tools/bench_usb_diagnose, tools/bench_usb_port_power, tools/bench_usb_topology, instruments/usb-hubs/troubleshooting]
instrument: usb-hubs
verified: n/a (procedure; the tools were run on the dev box but this exact sequence was not)
---

# USB measurement recipes

## Find a device that flaps

A device that disconnects and reconnects (a flaky cable, a weak hub port, a bus-powered unit browning out) shows up as many connects for one port in the kernel journal.

1. `bench_usb_history {"use_sudo": true}`: connects and disconnects per port, enumeration failures, error counters (`-71` protocol error and `-32` pipe error counts are the usual cable signs) and a flapping flag. Note the port path of the worst offender.
2. `bench_usb_topology`: which hub and which hub port that path is, and what else shares the hub. A bus-powered unit on a hub with many others is a power suspect.
3. `bench_usb_watch {"seconds": 120}` while wiggling the cable or running the device's workload: every change in the tree is reported with its time, so the trigger can be matched to the event.
4. `bench_usb_diagnose` on the device: ranked findings (speed fallback, a low-power hub, repeated resets) with next steps.
5. Change one thing and repeat step 1: another cable, a powered hub, another port. The flap count per hour before and after is the comparison. Where the hub can switch a port's power, `bench_usb_port_power` gives a clean cold start (a protected device refuses; see `bench_usb_protection`).

Read it as: errors that stop after a cable change were the cable; errors that follow the device to another port are the device; flaps that happen at the same moment as other devices on the same hub are the hub's power.

Verified status: none for this sequence as a whole.
