# hp_notebook_81ff

A Debian package, `hp-notebook-81ff-support`, for an **HP Notebook with system board 81FF** (product number
`1BQ18UA#ABA`, Core i7-7500U, Kaby Lake; BIOS F.22 of 2017-01-03) on Debian 13 with GNOME. It follows the pattern of
the other laptop packages in `hardware/`: it depends on `convertible-power` (TLP: maximum performance on AC, efficiency
on battery, UPower action at 10%) and adds this model's settings, applied only when `/sys/class/dmi/id/board_name` is
`81FF`.

Found on one machine (2026-10-29), kernel 6.12.113, Secure Boot on (kernel lockdown mode `integrity`), 7.6 GiB RAM, a
Toshiba MQ01ABD100 5400 rpm 2.5-inch SATA disk, Realtek RTL8188EE wireless (the only network in use) and RTL810xE Ethernet.

## The problem it fixes

The wireless adapter sits behind PCI Express root port `00:1c.5` (Intel `8086:9d15`). With ASPM L0s/L1 on, the kernel logged
about 270 correctable errors in 11 minutes (`PCIe Bus Error: severity=Correctable, type=Physical Layer, RxErr`, then
`AER: Correctable error message received`). That kept `systemd-journald` at about 23% of a CPU and the disk busy (6.6% wait on an
otherwise idle machine).

What was tried, in order:

* Writing the ASPM bits at run time (`setpci`, the `pcie_aspm` policy parameter): refused (`Operation not permitted`) under kernel
  lockdown.
* Driver options only (`rtl8188ee aspm=0 ips=0 fwlps=0 swlps=0`, in `modprobe.d/`): the options took effect (`aspm` reads 0) and the
  errors **continued** (263 lines in the first 90 s of a boot).
* `pcie_aspm=off` on the kernel command line: **0 errors** at boot and after a minute, and after traffic. This is what the
  package installs, through `/etc/default/grub.d/50-hp-notebook-81ff.cfg`, and it runs `update-grub`. It takes effect at the
  next boot.

The package keeps the driver options as well (harmless; the adapter was unreliable with its idle power saving).

Cost: with the kernel not managing ASPM, the package never reached a package C-state deeper than PC3 (87% in PC3 on AC, none in
PC6 to PC10) in a 20 s powertop run; with ASPM on, it was the same (81% PC3, 0% PC6+), so deeper idle was not available before either.

## Screen and lid

For a machine that is used as a build and test host, often with the lid shut: the screen turns off after 5 minutes
without input (`idle-delay`, a GNOME default for all users in `/etc/dconf/db/local.d/50-hp-notebook-81ff`); on AC the lid
switch is ignored (`HandleLidSwitchExternalPower=ignore` and docked, in `/etc/systemd/logind.conf.d/50-hp-notebook-81ff.conf`)
and the machine does not suspend from inactivity; on battery the lid suspends and 20 minutes idle suspends. logind reads its
drop-in at boot, so this takes effect at the next boot; a user's own GNOME settings override the dconf defaults.

## What else was checked

* **TRIM:** `fstrim.timer` is enabled by default on Debian 13 (weekly); the package enables it again to be sure. It does
  nothing for a hard disk and trims an SSD after the disk is replaced.
* **powertop** (2.15, 20 s runs, on AC): 182 wakeups/s; top sources `tick_nohz_handler` and the wireless driver
  (`cfg80211_wiphy_work`, `rtl_lps_change_work_callback`). Its suggestions (runtime PM for the Realtek devices, the SATA
  controller and the disk, audio codec power management, `nmi_watchdog=0`, 15 s VM writeback) are covered by TLP on battery through
  `convertible-power`. `SATA` link power management is already `med_power_with_dipm`.
* **latencytop:** not in Debian 13, and Debian kernels are built without `CONFIG_LATENCYTOP`; use `perf sched latency`,
  `bpftrace` or `systemd-analyze` instead.
* **Disk:** SMART passes (0 reallocated, 0 pending sectors, 2606 power-on hours, 9052 load cycles). The 5400 rpm disk is the
  remaining cause of slowness for anything disk-heavy; a 2.5-inch SATA SSD is a drop-in replacement.

## Not verified

* The battery-side settings (the machine was on AC). The battery reports `Full 100` and exposes `charge_full` (2796000 µAh),
  not `energy_full`.
* Whether the board name `81FF` covers other HP 15 models; only this machine was inspected.

Licence: GPL-3.0-or-later.
