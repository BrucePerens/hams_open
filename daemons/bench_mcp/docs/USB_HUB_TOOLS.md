# USB hub tools (bench_mcp, `usb_hub` driver)

The `bench_usb_*` tools describe and control the host's USB tree. They live in `src/usb_hub/` and are part of the bench_mcp server,
because the VNA (vector network analyzer) is attached through a power-switching hub: the VNA cold reset and the hub tools share one implementation of
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
| `bench_usb_bandwidth` | read_only | Translator, bus and controller sharing with periodic bandwidth now and worst case, USB 2 and USB 3 companion root hubs. |
| `bench_usb_diagnose` | read_only | Why a device is slow, misbehaving or missing: ranked findings with evidence and a next step. |

The topology and find tools read `/sys/bus/usb/devices` and need no privilege. `bench_usb_hubs` and `bench_usb_port_power` use `sudo -n`; without a NOPASSWD rule, or
without uhubctl, they fail with a message that says which.

### `usb_hub_watch_with`

`fn watch_with(sysfs: &Path, args: &Value, after_poll: &mut dyn FnMut(u64) -> bool) -> Result<Value, String>` in
`src/usb_hub/history.rs`. It is the loop behind `bench_usb_watch`; `history::watch(sysfs, args)` calls it with a callback that
never stops. It is private to the module.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path` | Root of the USB tree to poll (`/sys` in the tool, a writable copy of recorded sysfs in tests). Borrowed, read only. |
| `args` | `&Value` | `seconds` (1 to 120, default 10; anything else is an error) and `interval_ms` (50 to 5000, default 200, clamped). |
| `after_poll` | `&mut dyn FnMut(u64) -> bool` | Called on the watching thread once after the first snapshot with 0 and after every later poll with 1, 2, ...; returning true ends the watch early. Borrowed mutably for the call. |

**Returns:** `Ok` with `watched_s`, `started_unix`, `poll_ms`, `events` (appeared, disappeared, re-enumerated and port changes, each
with its time), `enumerations_per_device`, `flapping` (devices that appeared three times or more) and a `method` note. `Err` with a
message when `seconds` or `interval_ms` is not a number or `seconds` is out of range, or when the tree cannot be read.

**Concurrency and side effects:** blocking; it sleeps one interval between polls on the calling thread and takes no lock. It only
reads the tree. A change shorter than the interval can still be missed when the machine is slow, which the `method` note says;
the kernel journal (`bench_usb_history`) is exact. The callback runs inside the loop, so a callback that changes the tree (the
test's fake hub) changes what the next poll sees. Test: `a_watch_reports_a_device_leaving_and_returning_with_a_new_number`.

## Privilege for the middle tier

Resetting, authorizing, binding and autosuspend need root. The tools run `sudo -n <bench_mcp binary> --usb-priv <operation>`; that mode of the same binary accepts only the four
operations (`reset`, `authorize`, `unbind`/`bind`, `autosuspend`), checks every argument against the real sysfs (a device path of a device behind a port, an interface name, an installed
USB driver, bounded numbers) and does nothing else. It is a different route from `sudo -n uhubctl`, which only the port power tool uses.
A sudoers rule must name the binary with `--usb-priv *`, and the binary must be root-owned and not writable by the user.

## Verification levels of port power control

A port cannot be verified from software alone, so the output gives a level, never a boolean: 1 claimed (uhubctl reports per-port switching); 2 re-enumerated (the device left the tree
when the port went off and came back with a new device number and readable descriptors, with the measured times; checked on every cycle the tool performs); 3 state cleared (a
device-specific check, recorded in the config); 4 VBUS (the USB power line) measured (manual entry in the config). Level 2 shows the port dropped the device, not that VBUS fell: some hubs disable the
data lines and leave VBUS on. The VNWA on hub 3-2 port 2 is confirmed by eye to go dark and keep nothing, which the config records as `confirmed_by_hand` (level 2 plus manually confirmed).

## Safety rules

* Only ports of hubs uhubctl lists as per-port switching can be switched (ganged and unlisted hubs are refused). A SuperSpeed hub is addressed with `-e`.
* Refused by default: the device carrying the root filesystem or another mounted USB filesystem, boot-protocol keyboards and mice, config-critical ports and devices, the serial adapter of a bench instrument, the hub port of a configured VNA with everything below it and the VNA's USB id (overridable), any hub on the path to those, and the uplink of any hub (unless the port has `hub_uplink_ok`). Reasons from the config and from bench instruments cannot be overridden; the others can with `override_protection: true`.
* `dry_run: true` on a mutating tool says what would happen (commands, devices affected, protection verdict) and changes nothing.
* The off time is a parameter (1 to 60 s, default 5 s); the actual off time is reported.
* A failed power-on after a power-off is retried once and, if it still fails, the error says the port is still off.

The VNA's cold reset (`bench_vna_power_cycle` and the recovery inside it) calls the same `usb_hub::power` code, with the VNA's own off times and "unit and both codecs present" as its ready check; its result includes the re-enumeration report.

The same text, organised for the MCP manual, is in `manual/instruments/usb-hubs/` and `manual/tools/`.

## Worked example: the VNWA chain

The VNWA unit is a full-speed hub (3-2.2) with its control chip and two USB audio codecs, on port 2 of the high-speed hub 3-2. `bench_usb_bandwidth` shows the four devices behind one translator
(the one of 3-2 port 2), with a worst-case periodic load near 60 % of its 1350 bytes/ms; `bench_usb_topology` shows each negotiated at 12 Mb/s with a note that the codecs report USB 2.00; and `bench_usb_history`
shows the unit's re-enumerations after each power cycle and the enumeration failures (-71, -32) the control chip once had after a short power-off.
