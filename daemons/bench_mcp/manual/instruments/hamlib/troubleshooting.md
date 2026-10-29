---
title: Hamlib troubleshooting
summary: Symptom, cause and next action for a rig that does not answer, a daemon that will not start, and wrong values.
see_also: [instruments/hamlib/limits, concepts/errors, concepts/extending/anatomy]
instrument: hamlib
verified: dummy rig only, 2026-10-29
---

# Hamlib troubleshooting

In escalation order: retry the read, restart the daemon, power-cycle the rig's USB port with the hub tools, ask the person at the bench.

| Symptom | Cause | Next action |
|---|---|---|
| `rigctld is not installed` | the Hamlib utilities are missing | install `libhamlib-utils` |
| `the rig's device ... cannot be used` | the path does not exist; the rig is unplugged or the tty moved | check the cable; use `bench_usb_find` and a `/dev/serial/by-id` path |
| `may not read and write` | the user is not in the device's group | add the user to `dialout` (or the group shown) and log in again |
| `RPRT -5` or `-6`, or a timeout | the rig is off, the baud rate or model is wrong, the cable or the interface is faulty | check power and the `speed` and `model` keys; then `bench_hamlib_rigctld` restart; then power-cycle the USB port (`bench_usb_port_power`, only a port the owner marked) |
| `RPRT -11` or `-4` | the rig or backend does not offer the function | `bench_hamlib_models` with the model shows what it offers; this is not a fault |
| `rigctld ... has been started 4 times in 10 minutes` | the daemon keeps dying | read the log in the managed directory (`<rig>.rigctld.log`), fix the cause, then `bench_hamlib_rigctld` start |
| `managed by another bench_mcp process` | two servers use one rig | use that server, or stop it |
| values look stale or wrong | many rigs cache a value for a moment; the rig's panel may disagree after a knob turn | read again; compare with the rig's display |
