---
title: bench_usb_hubs
summary: Hub capabilities, per-port status and power-control verification levels.
see_also: [instruments/usb-hubs/overview, instruments/usb-hubs/operation]
tools: [bench_usb_hubs]
instrument: usb-hubs
tier: read_only
verified: see usb-hubs/operation
---

# bench_usb_hubs

Hub capabilities, per-port status and power-control verification levels.

Arguments: `hub`, `privileged` (false skips the sudo reads). Runs `sudo -n uhubctl` and `sudo -n lsusb -v`; sysfs facts are given even when they fail.

See the USB hub pages for the verification levels and the protection rules.

## Arguments

<!-- arguments -->
