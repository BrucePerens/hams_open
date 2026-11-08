---
title: Glasgow troubleshooting
summary: Symptom, cause and next action in escalation order, including the full-speed USB link seen on the first unit.
see_also: [instruments/glasgow/setup, instruments/glasgow/limits, tools/bench_glasgow_safe_state]
instrument: glasgow
verified: measured on the unit (Glasgow revC3, device firmware API level 3, glasgow software 70cb38c9), 2026-11-08
---

# Glasgow troubleshooting

| Symptom | Cause | Next action |
|---|---|---|
| `missing permissions to open device` | no udev rule, or the device re-enumerated and lost a hand-set ACL | install the rule ([setup](instruments/glasgow/setup)) |
| `linked at 12 Mb/s (full speed)` and applets with data streams refuse to run | the USB link negotiated full speed, not high speed. Measured on the first unit (2026-11-08): on a 480 Mb/s root hub port, with two cables, on two controllers, the kernel reported speed 12 before and after loading the software's firmware; `glasgow run benchmark` sink mode ran at 0.91 MiB/s and source and latency modes failed with `LIBUSB_TRANSFER_OVERFLOW`, because the 512-byte bulk endpoints are invalid at full speed (the kernel logs `invalid maxpacket 512, setting to 64`) | try a short (under 1 m) cable known to carry USB 2.0 data, a rear port of a USB 2.0 root hub, then a powered USB 2.0 hub (it does its own high-speed handshake) and another computer; if it still links at 12 Mb/s on all of them the board's USB high-speed handshake is the suspect: that is a hardware problem for the owner. Register-only applets (`bench_glasgow_gpio`, `_pattern`) work at full speed |
| `please run glasgow flash to update firmware` (in `log_problems`) | the device holds older firmware than the software; the software loads its own into RAM each time | nothing: no tool flashes. Updating the EEPROM is a person's job with `glasgow flash` |
| device node changes after each call | the FX2 re-enumerates when firmware is loaded or a transfer fails | normal; the udev rule keeps access |
| `did not finish within N s` | a first-use gateware build is slow on a loaded machine, or the target never answers | `bench_glasgow_prepare`, then retry; then `bench_glasgow_safe_state` |
| `port A is not as it was set` | something else changed the voltage, or a fault tripped | the ports were switched off by the server; look for a short and for another process using the device |
| `SAFE STATE NOT CONFIRMED` | the device does not answer | unplug the USB cable |
| `did not end after SIGINT and SIGTERM` | a `glasgow` child is wedged | look at the pid named in the message and end it by hand; the server never sends SIGKILL |
