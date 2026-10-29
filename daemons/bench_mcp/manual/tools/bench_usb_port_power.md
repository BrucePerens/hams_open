---
title: bench_usb_port_power
summary: Switch a hub port off, on or cycle it, with re-enumeration checked and dry-run.
see_also: [usb-hubs/overview, usb-hubs/operation]
tools: [bench_usb_port_power]
instrument: usb-hubs
tier: full
verified: see usb-hubs/operation
---

# bench_usb_port_power

Switch a hub port off, on or cycle it, with re-enumeration checked and dry-run.

Arguments: `action` (off, on, cycle), the port (`hub`+`port`, `label`, `device`), `off_time_s`, `back_timeout_s`, `expect_vidpid`, `dry_run`, `override_protection`. off and cycle need tier full; on needs read_config.

See the USB hub pages for the verification levels and the protection rules.
