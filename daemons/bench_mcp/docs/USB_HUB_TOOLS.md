# USB hub tools (bench_mcp, `usb_hub` driver)

The `bench_usb_*` tools describe and control the host's USB tree. They live in `src/usb_hub/` and are part of the bench_mcp server,
because the VNA is attached through a power-switching hub: the VNA cold reset and the hub tools share one implementation of
power off, power on and the wait for re-enumeration.

## Registering

The tools belong to one instrument entry of the bench config (`BENCH_MCP_CONFIG`) with `"driver": "usb_hub"`. The entry's tier
decides what is listed: `read_only` lists the information tools, `read_config` adds the middle tier, `full` adds port power off and cycling.

```json
{"instruments": {
  "usb": {"driver": "usb_hub", "tier": "read_only",
          "ports": {"3-2/2": {"label": "VNWA 3E", "hub_uplink_ok": true,
                              "confirmed_by_hand": "goes dark and keeps no settings"}},
          "critical_devices": ["0451:2036"]}
}}
```

MCP client registration is the same as for the rest of bench_mcp (a stdio server, `BENCH_MCP_CONFIG` in its environment).
`USB_HUB_SYSFS` points the readers at another sysfs root (a recording); it is for inspection of captured trees and is unset in use.

* `ports`: key is `<hub path>/<port>` (`3-2/2`, or `usb3/4` for a root-hub port). Fields: `label` (shown in listings and accepted wherever a device is
  named; unique, case-insensitive), `critical` (never switched off or reset by a tool), `hub_uplink_ok` (the hub plugged into this port is the intended target),
  `confirmed_by_hand`, `state_cleared`, `vbus_measured` (verification levels, see below).
* `critical_devices`: `vid:pid` values that are critical wherever they are plugged in.

## Tools

| Tool | Tier | What it does |
| --- | --- | --- |
| `bench_usb_topology` | read_only | The tree from sysfs, no root: bus, controller, hub path and port, label, ids, strings, class, USB version, negotiated speed and a finding when it is below the device's capability, bMaxPower, authorized, autosuspend state, interfaces with drivers and the nodes they expose; for hubs the TT mode and per-port state. |
| `bench_usb_find` | read_only | Which device is `/dev/ttyUSB0` (or a sound card id, a hidraw, a network interface, a vid:pid, a product, a label). |

Speed findings: a USB 3 device running below SuperSpeed is a `warn`; a device that reports USB 2.x but runs at 12 Mb/s is a `note` only, because many
full-speed devices report 2.0 in `bcdUSB`.

Everything above reads `/sys/bus/usb/devices` and needs no privilege. The tools that need privilege are documented with their tiers as they are added.
