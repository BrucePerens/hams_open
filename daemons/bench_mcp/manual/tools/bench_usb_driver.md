---
title: bench_usb_driver
summary: Unbind an interface from its driver, or bind it to a named driver.
see_also: [instruments/usb-hubs/overview, instruments/usb-hubs/operation]
tools: [bench_usb_driver]
instrument: usb-hubs
tier: read_config
verified: see usb-hubs/operation
---

# bench_usb_driver

Unbind an interface from its driver, or bind it to a named driver.

Arguments: `interface` (like `3-2.2.2:1.3`), `action` (unbind, bind), `driver` (needed for bind), `dry_run`, `override_protection`. The dry run lists the nodes (tty, sound card, input) that would go away. Tested against a copy of sysfs; not run on hardware.

See the USB hub pages for the protection rules.

## Arguments

<!-- arguments -->
