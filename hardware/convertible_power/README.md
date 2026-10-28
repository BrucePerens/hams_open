# convertible_power

The package `convertible-power` holds the power settings shared by the device packages in this family
(`panasonic-cf19-support`, `panasonic-fz-g1-support`, `hp-spectre-x360-16-support`): **maximum performance on AC
power, maximum battery life on battery**, applied automatically when the charger is plugged or unplugged.

It uses **TLP**, which is `powertop --auto-tune` done properly: the same runtime power management, SATA link power,
Wi-Fi power saving, USB autosuspend and audio power saving, but chosen separately for AC and battery and applied
again at every change. TLP and `power-profiles-daemon` cannot run together, so installing this package removes
`power-profiles-daemon`; GNOME's "Power Mode" switch goes away (there is nothing to switch).

## Settings (`/etc/tlp.d/10-convertible-power.conf`)

| | On AC | On battery |
|---|---|---|
| CPU governor | `performance` | set per device (the valid names depend on the CPU frequency driver) |
| Energy/performance preference (CPUs with HWP) | `performance` | `power` |
| Turbo / HWP dynamic boost | on | off |
| Platform profile (where the firmware has one) | `performance` | per device |
| PCI and AHCI runtime power management | off (full speed) | `auto` |
| Wi-Fi power saving | off | on |
| SATA link power | `max_performance` | `med_power_with_dipm` |
| Disk APM level | 254 | 128 |
| Audio codec power saving | off | on |
| NMI watchdog | off | off |
| Wake-on-LAN | left as the hardware has it | left as the hardware has it |
| USB autosuspend | on | on (the device packages exclude input and sensor devices) |

Each device package adds `/etc/tlp.d/20-<device>.conf`, which TLP reads after this file. Local changes belong in
`/etc/tlp.conf`, which wins over both. `tlp-stat` shows what is in effect.

The powertop tunables that matter are covered by TLP's defaults plus the table above; `powertop` itself is a
recommended package, to look at what is still using power. Settings that cannot be applied (a governor the driver
does not have, a profile the firmware lacks) are skipped by TLP with a message, not treated as errors.

## Charge management: 80% maximum, 10% minimum

* **Hardware limit (80%).** The package sets TLP's charge thresholds (start below 75%, stop at 80%). They take
  effect only where the firmware exposes `charge_control_*_threshold`, and none of the three machines checked does:
  the HP Spectre x360 16-f2xxx (the BIOS option "Battery Health Manager" is not offered through `hp-bioscfg`, which
  exposes only Sure Start), and the Toughpad FZ-G1, whose firmware has no charge-limit handler in its ACPI tables
  (checked 2026-10-28; the `eco_mode` attribute of `panasonic_laptop` returns an error there). The CF-19 has not been
  checked.
* **Reminder (80%).** Where the firmware cannot stop charging, `convertible-battery-notify` (a user service) shows one
  notification per charge when the battery reaches the maximum in `/etc/convertible-power/battery.conf`, asking you to
  unplug. It stays silent when a hardware limit at or below the maximum exists.
* **Minimum (10%).** `/etc/convertible-power/UPower.conf`, used by UPower through a drop-in for `upower.service`
  (`UPOWER_CONF_FILE_NAME`, so `/etc/UPower/UPower.conf` is not touched), warns at 20%, critically at 12%, and at 10%
  does the critical action: hibernate (UPower falls back to power-off where hibernation is not set up).

To change the numbers, edit the three places named in `battery.conf`.

## Not verified

The behaviour on battery (the machines were on AC during the work): the settings were applied and read back
(`tlp-stat`), but no discharge-rate measurement was made. The CF-19 has not been tried with this package.

Licence: GPL-3.0-or-later.
