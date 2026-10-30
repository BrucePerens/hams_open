---
title: USB hub tool protocol notes
summary: What the USB tools read and run: sysfs, uhubctl, lsusb, journalctl and the root helper, and where each fact came from.
see_also: [instruments/usb-hubs/setup, instruments/usb-hubs/limits, concepts/extending/provenance]
instrument: usb-hubs
verified: code only, 2026-10-29
---

# USB hub tool protocol notes

The USB tools take no command string from the caller. They read the kernel's view and run a fixed set of programs with arguments built from checked values (a hub location, a port number, an interface name made of digits, dots, dashes and colons).

| What | How | Notes |
|---|---|---|
| tree, speeds, drivers, descriptors | files under `/sys/bus/usb/devices` and `/sys/devices` (`busnum`, `devnum`, `idVendor`, `idProduct`, `speed`, `authorized`, interface and driver links) | read only; the tests replay a recorded tree |
| hub switching capability and port status | `sudo -n uhubctl` (list), and with `-N -l <hub> -p <port> -a off\|on\|cycle` (plus `-e` for an exact port match) to switch | needs the sudoers rule in the setup page; the answer says so when it is missing |
| hub descriptor claims | `sudo -n lsusb -v -s <bus>:<dev>` | read only |
| history and flapping | `journalctl -k` for the kernel's USB lines (`use_sudo` when the user may not read it) | read only |
| soft reset, deauthorize, authorize, unbind, bind | the root helper: `sudo -n <bench_mcp binary> --usb-priv <operation> ...` | accepts only the operations `reset <node>` (the `USBDEVFS_RESET` ioctl, 0x5514), `authorized <device> 0\|1`, and `bind\|unbind <interface> <driver>`, each checked against the sysfs tree |

Every tool that can drop a device takes `dry_run` and refuses the protected devices first. The VNA's hub port is protected, and the cycle of it is done only by the VNA tool.

| Fact | Source kind | Source |
|---|---|---|
| uhubctl options and its output format | other public documentation | the uhubctl project's manual and its output as seen on the dev box |
| sysfs attribute names | other public documentation | the Linux kernel's USB sysfs documentation |
| the recorded tree and outputs | observation of the device's replies | `tests/usb_fixtures/devbox/`, serial numbers replaced by `SERIAL-REDACTED` |
| `USBDEVFS_RESET` | other public documentation | the kernel's `usbdevice_fs.h` constant |

Nothing was copied from vendor source, firmware, drivers or help text.
