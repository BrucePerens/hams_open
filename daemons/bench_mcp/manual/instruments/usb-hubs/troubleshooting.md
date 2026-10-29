---
title: USB hub tools troubleshooting
summary: Errors from sudo and uhubctl, a port that does not drop its device, and a cycle that does not come back.
see_also: [instruments/usb-hubs/limits, instruments/usb-hubs/operation]
tools: [bench_usb_hubs, bench_usb_port_power]
instrument: usb-hubs
tier: read_only
verified: error texts exercised against scripted runs and the real uhubctl, 2026-10-29
---

# USB hub tools troubleshooting

* `sudo -n uhubctl needs a password`: add a NOPASSWD sudoers rule for uhubctl (and lsusb) for the user that runs bench_mcp.
* `uhubctl is not installed`: install uhubctl; `bench_usb_hubs` still gives the sysfs facts.
* `the device stayed in the USB tree ... after the port was switched off`: the hub accepted the command but the port did not drop the device. Do not rely on that port for a power clear.
* `the device did not come back within N s`: the port may be left OFF if the error says so; switch it on with `action: on`. A longer `off_time_s` has brought back a unit that failed to enumerate after a short off time (the VNA cold reset retries with 12 s itself).
* `refused: ...`: see `bench_usb_protection`. Reasons from the config cannot be overridden by an argument.
