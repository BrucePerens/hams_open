---
title: bench_usb_autosuspend
summary: Runtime power management of one device: control on (never suspend) or auto (after delay_ms).
see_also: [usb-hubs/overview, usb-hubs/operation]
tools: [bench_usb_autosuspend]
instrument: usb-hubs
tier: read_config
verified: see usb-hubs/operation
---

# bench_usb_autosuspend

Runtime power management of one device: control on (never suspend) or auto (after delay_ms).

Arguments: `device`, `control`, `delay_ms` (-1 to 3600000), `dry_run`, `override_protection`. Setting auto on a protected device is refused. Verified on the fingerprint reader.

See the USB hub pages for the protection rules.
