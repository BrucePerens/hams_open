---
title: Registering and configuring the USB hub tools
summary: The usb_hub entry of the bench config: port labels, critical ports and devices, and the sudo rule uhubctl needs.
see_also: [overview, operation]
tools: [bench_usb_hubs, bench_usb_port_power]
instrument: usb-hubs
tier: read
verified: config parser tested; sudo -n uhubctl and lsusb used on the dev box, 2026-10-29
---

# Setup

Add an instrument with `"driver": "usb_hub"` to the bench config (`BENCH_MCP_CONFIG`):

```json
{"instruments": {
  "usb": {"driver": "usb_hub", "tier": "read_only",
          "ports": {"3-2/2": {"label": "VNWA 3E", "hub_uplink_ok": true,
                              "confirmed_by_hand": "goes dark and keeps no settings"}},
          "critical_devices": ["0451:2036"]}
}}
```

* `ports`: key `<hub path>/<port>` (`3-2/2`, or `usb3/4` for a root-hub port). `label` is shown in listings and accepted wherever a device or port is named (unique, case-insensitive).
  `critical` makes the port never switchable or resettable by a tool. `hub_uplink_ok` says the hub plugged into the port is the intended target of power control (the VNWA's own hub is). `confirmed_by_hand`, `state_cleared` and `vbus_measured` record verification levels 2, 3 and 4 (see operation).
* `critical_devices`: `vid:pid` values that are critical wherever they are plugged in.
* Power tools need passwordless `sudo -n uhubctl` (and `sudo -n lsusb` for the hub descriptor). Without it the tools say so: `sudo -n uhubctl needs a password: add a NOPASSWD sudoers rule`, or `uhubctl is not installed`.
* The bench instruments' serial adapters (the `serial` of a power supply entry) are protected automatically.

**Root helper.** The middle-tier tools need root to write sysfs and to send USBDEVFS_RESET. They run `sudo -n <path of the bench_mcp binary> --usb-priv <operation> ...`. That mode accepts only
`reset <device>`, `authorize <device> 0|1`, `unbind|bind <interface> <driver>` and `autosuspend <device> on|auto <ms|->`, validates every argument against the real sysfs, and does nothing else.
A sudoers rule `user ALL=(root) NOPASSWD: /path/to/bench_mcp --usb-priv *` allows it; the binary must be owned by root and not writable by the user, or the rule lets the user run anything as root.
