---
title: bench_usb_authorize
summary: Deauthorize (software disconnect that keeps the power) or authorize a USB device.
see_also: [instruments/usb-hubs/overview, instruments/usb-hubs/operation]
tools: [bench_usb_authorize]
instrument: usb-hubs
tier: read_config
verified: see usb-hubs/operation
---

# bench_usb_authorize

Deauthorize (software disconnect that keeps the power) or authorize a USB device.

Arguments: `device`, `authorized`, `dry_run`, `override_protection`. Authorizing is never refused. Verified on the fingerprint reader: deauthorized, then authorized again.

See the USB hub pages for the protection rules.

## Arguments

<!-- arguments -->
