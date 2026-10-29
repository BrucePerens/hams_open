---
title: bench_usb_reset
summary: Soft reset of one USB device (USBDEVFS_RESET); it keeps its power and device number.
see_also: [instruments/usb-hubs/overview, instruments/usb-hubs/operation]
tools: [bench_usb_reset]
instrument: usb-hubs
tier: read_config
verified: see usb-hubs/operation
---

# bench_usb_reset

Soft reset of one USB device (USBDEVFS_RESET); it keeps its power and device number.

Arguments: `device` (path or port label), `dry_run`, `override_protection`. Needs root through the `--usb-priv` helper (see setup). First rung of the ladder: reset, deauthorize/authorize, driver unbind/bind, then `bench_usb_port_power`. Verified on the dev box on a fingerprint reader (06cb:00ff): back in 0.37 s with the same device number.

See the USB hub pages for the protection rules.

## Arguments

<!-- arguments -->
