# lenovo_thinkpad_t430

A Debian package, `lenovo-thinkpad-t430-support`, for a **Lenovo ThinkPad T430** (machine type-model 23426QU, BIOS G1ET98WW
2.58, Core i5-3230M, 11 GB RAM, HGST HTS725050A7E630 hard disk, DVD-RAM drive) on Debian 13 with GNOME. It is the **demo laptop**
(ssh alias `thinkpad-i5`); it is not a lint, test, build or CI host. It follows the pattern of the other laptop packages in
`hardware/`: it depends on `convertible-power` (TLP; 1.0.3 or later, which also stops the GDM login screen from suspending on AC)
and adds this model's settings, applied only when `/sys/class/dmi/id/product_version` is `ThinkPad T430`.

## No quirk was needed

Measured 2026-11-05, kernel 6.12.115 (Debian), stock firmware packages (`firmware-iwlwifi`, `firmware-intel-graphics`):

| Device | State |
|---|---|
| Intel HD Graphics 4000 (i915), LVDS panel, backlight (`intel_backlight`) | works |
| Intel Centrino Wireless-N 2200 (iwldvm, firmware 18.168.6.1) | works; no firmware errors in dmesg |
| Intel 82579LM Ethernet (e1000e) | driver loaded, no cable tested |
| Audio: Realtek ALC3202 and HDMI/DP codec (snd_hda_intel) | works; PipeWire sink "Built-in Audio Analog Stereo", 440 Hz test tone played at low volume |
| Webcam Chicony 04f2:b2da (uvcvideo) | works; three frames captured with `v4l2-ctl` |
| Ricoh SD/MMC reader (sdhci-pci) | controller bound as `mmc0`; no card tested |
| TrackPoint (3 buttons) and touchpad | detected (psmouse) |
| thinkpad_acpi: hotkeys, fan (auto, 2574 rpm), battery | works |
| Battery charge thresholds | `charge_control_{start,end}_threshold` exist; 75/80 set and read back |
| DVD-RAM drive HL-DT-ST GT80N | detected as `sr0` (24x, DVD-RAM); no disc tested |
| HDMI, DisplayPort, VGA outputs | connectors present (`card0-HDMI-A-1..3`, `DP-1..3`, `VGA-1`), all disconnected; nothing plugged in to test |
| Bluetooth | **not present**: no Bluetooth device on USB, no `/sys/class/bluetooth`, no `/proc/acpi/ibm/bluetooth` (the module is an option on the T430). `bluez` is installed, so a USB dongle works |
| Fingerprint reader | **not present**: none on USB (the reader is an option). `fprintd` is installed |
| ExpressCard / USB 3 | USB 3 xHCI (`xhci_hcd`) works; no ExpressCard slot controller found in `lspci` |
| TPM | `ima: Error Communicating to TPM chip` in dmesg at boot: harmless, no TPM present or enabled |
| ACPI thermal | `[Firmware Bug] Invalid critical threshold`, `No valid trip points` in dmesg: harmless, `thinkpad_acpi` and `thermald` handle temperature |

The kernel messages above are the only warnings; none needed a kernel parameter or module option.

## What the package sets

* `/etc/tlp.d/20-lenovo-thinkpad-t430.conf`: governor `performance` on AC, `schedutil` on battery (the CPU runs
  `intel_cpufreq`, which offers only `performance` and `schedutil`, not `powersave`); charge thresholds 75/80 (a demo laptop
  sits on the charger; use 40/80 for one that travels); Wi-Fi power saving off on battery too.
* `/etc/systemd/logind.conf.d/50-lenovo-thinkpad-t430.conf`: lid switch ignored on AC power and when docked, suspends on battery.
* `/etc/dconf/db/local.d/50-lenovo-thinkpad-t430`: screen off after 300 s idle; no inactivity suspend on AC; 20 minutes on battery.
* `/etc/NetworkManager/conf.d/50-lenovo-thinkpad-t430.conf`: `wifi.powersave=2` (off) and `connection.autoconnect-retries=0`
  (unlimited) as defaults for every connection. The existing connection `Bruce` was also set explicitly with `nmcli`.

The login screen's no-suspend-on-AC setting comes from `convertible-power` (`/usr/share/gdm/dconf/95-convertible-power`).
Verified with the greeter's dconf profile (`DCONF_PROFILE=gdm gsettings get ... sleep-inactive-ac-type` returns `'nothing'`); a
20-minute idle soak at the login screen was not run, because a user session was logged in.

## Not verified

Behaviour on battery; HDMI/VGA/DP projection; SD card, DVD disc and Ethernet with real media; suspend and resume on lid close
(the lid is ignored on AC by design; battery suspend not tried).

## BIOS

Installed G1ET98WW (2.58). A web search on 2026-11-05 shows Lenovo's download page listing 2.59 (G1ET99WW) above 2.58, and forum
posts mention later builds, so 2.58 is **not** confirmed to be the last. Nothing was flashed. fwupd cannot update it (the machine
boots in legacy BIOS mode and has no LVFS entry for the BIOS).

Licence: GPL-3.0-or-later.
