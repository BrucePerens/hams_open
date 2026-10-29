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

| `bench_usb_hubs` | read_only | Per hub: ports, TT mode, uplink, what uhubctl says about switching, the hub class descriptor (power-switching mode, over-current mode, TT think time, as the hub claims them), per-port status bits and the verification level. Runs `sudo -n uhubctl` and `sudo -n lsusb -v`. |
| `bench_usb_protection` | read_only | The protection list with reasons, and the verdict for one target. |
| `bench_usb_port_power` | read_config (on), full (off, cycle) | Switch one port off, on or cycle it; `dry_run`, `off_time_s` (default 5 s), re-enumeration check, verification level. |
| `bench_usb_reset` | read_config | Soft reset (USBDEVFS_RESET) of one device; it keeps power and device number. |
| `bench_usb_authorize` | read_config | Deauthorize (software disconnect, power kept) or authorize a device. |
| `bench_usb_driver` | read_config | Unbind or bind one interface. |
| `bench_usb_autosuspend` | read_config | Runtime power management on or auto. |
| `bench_usb_history` | read_only | Timeline and counters from `journalctl -k -o short-unix` (errno counts, resets, enumeration failures, over-current), flapping detection. The journal needs root or the group systemd-journal/adm; `use_sudo: true` opts in to `sudo -n journalctl`. |
| `bench_usb_watch` | read_only | Watch the tree for up to 120 s by polling sysfs; every appear, disappear, re-enumeration and port change with its time. |

The topology and find tools read `/sys/bus/usb/devices` and need no privilege. `bench_usb_hubs` and `bench_usb_port_power` use `sudo -n`; without a NOPASSWD rule, or
without uhubctl, they fail with a message that says which.

## Privilege for the middle tier

Resetting, authorizing, binding and autosuspend need root. The tools run `sudo -n <bench_mcp binary> --usb-priv <operation>`; that mode of the same binary accepts only the four
operations (`reset`, `authorize`, `unbind`/`bind`, `autosuspend`), checks every argument against the real sysfs (a device path of a device behind a port, an interface name, an installed
USB driver, bounded numbers) and does nothing else. It is a different route from `sudo -n uhubctl`, which only the port power tool uses.
A sudoers rule must name the binary with `--usb-priv *`, and the binary must be root-owned and not writable by the user.

## Verification levels of port power control

A port cannot be verified from software alone, so the output gives a level, never a boolean: 1 claimed (uhubctl reports per-port switching); 2 re-enumerated (the device left the tree
when the port went off and came back with a new device number and readable descriptors, with the measured times; checked on every cycle the tool performs); 3 state cleared (a
device-specific check, recorded in the config); 4 VBUS measured (manual entry in the config). Level 2 shows the port dropped the device, not that VBUS fell: some hubs disable the
data lines and leave VBUS on. The VNWA on hub 3-2 port 2 is confirmed by eye to go dark and keep nothing, which the config records as `confirmed_by_hand` (level 2 plus manually confirmed).

## Safety rules

* Only ports of hubs uhubctl lists as per-port switching can be switched (ganged and unlisted hubs are refused). A SuperSpeed hub is addressed with `-e`.
* Refused by default: the device carrying the root filesystem or another mounted USB filesystem, boot-protocol keyboards and mice, config-critical ports and devices, the serial adapter of a bench instrument, the hub port of a configured VNA with everything below it and the VNA's USB id (overridable), any hub on the path to those, and the uplink of any hub (unless the port has `hub_uplink_ok`). Reasons from the config and from bench instruments cannot be overridden; the others can with `override_protection: true`.
* `dry_run: true` on a mutating tool says what would happen (commands, devices affected, protection verdict) and changes nothing.
* The off time is a parameter (1 to 60 s, default 5 s); the actual off time is reported.
* A failed power-on after a power-off is retried once and, if it still fails, the error says the port is still off.

The VNA's cold reset (`bench_vna_power_cycle` and the recovery inside it) calls the same `usb_hub::power` code, with the VNA's own off times and "unit and both codecs present" as its ready check; its result includes the re-enumeration report.

The same text, organised for the MCP manual, is in `manual/instruments/usb-hubs/` and `manual/tools/`.
