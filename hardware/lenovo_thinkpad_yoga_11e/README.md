# lenovo_thinkpad_yoga_11e

A Debian package, `lenovo-thinkpad-yoga-11e-support`, for a first-generation **Lenovo ThinkPad Yoga 11e** convertible
(machine type-model 20D9CTO1WW, BIOS N15ET21W 1.07 of 2014-06-13, Celeron N2930 "Bay Trail", 4 GB RAM, Seagate ST320LM010
320 GB hard disk, 1366x768 touchscreen) on Debian 13 with GNOME 48. It is the **demo laptop** `bruce-11e` (ssh alias
`bruce-11e`); it is not a lint, test, build or CI host. It follows the pattern of the other laptop packages in `hardware/`:
it depends on `convertible-power` (TLP; 1.0.3 or later, which also stops the GDM login screen from suspending on AC) and
`convertible-helper` (1.4.0 or later: the screen is set upright when the laptop leaves tablet mode, and the on-screen
keyboard is enabled at the login screen), and adds this model's settings, applied only when
`/sys/class/dmi/id/product_version` is `ThinkPad Yoga 11e`.

## No quirk was needed

Measured 2026-11-05, kernel 6.12.115 (Debian), stock firmware packages (`firmware-iwlwifi`, `firmware-intel-graphics`,
`firmware-realtek`):

| Device | State |
|---|---|
| Intel HD Graphics (Bay Trail, i915), eDP panel, backlight (`intel_backlight`, max 7812) | works. `[drm] *ERROR* Failed to write source OUI` three times at boot is a harmless eDP DPCD write the panel does not support |
| Touchscreen Atmel ATML1000 (03EB:8C1F, I2C HID, `hid-multitouch`) | works |
| Sensor hub ITE8386 (048D:8380, I2C HID, `hid-sensor-hub`) | works: `accel_3d`, `gyro_3d`, `dev_rotation`, `magn_3d`, `incli_3d` IIO devices; `iio-sensor-proxy` reports `HasAccelerometer=true`, orientation `normal` at the desk. `i2c_hid_acpi i2c-ITE8386:00: device did not ack reset within 1000 ms` at boot is harmless (the hub enumerates right after) |
| Tablet-mode switch | `thinkpad_acpi: Tablet mode switch found (type: GMMS)`; `SW_TABLET_MODE` on "ThinkPad Extra Buttons". GNOME rotates the screen from the accelerometer in tablet mode and shows the on-screen keyboard |
| Pen | **not present** (the first-generation Yoga 11e has no digitizer) |
| Intel Wireless 7260 (iwlmvm, firmware 17.bfb58538.0) | loads with no errors; the machine was on Ethernet during the test |
| Intel Bluetooth 8087:07dc | works (`hci0`, already patched by the ROM) |
| Realtek RTL8111 Ethernet (r8169) | works (the connection used for setup). `can't disable ASPM; OS doesn't have ASPM control` is harmless |
| Audio: HDA Intel (snd_hda_intel) | works; PipeWire sink "Built-in Audio Analog Stereo". The ThinkPad EC console audio control (mute LED) is card 29 |
| Webcam Chicony 04f2:b451 (uvcvideo) | works; three 640x480 YUYV frames captured with `v4l2-ctl` |
| Synaptics touchpad (s3203, psmouse) and keyboard | detected |
| thinkpad_acpi: hotkeys, fan (auto), battery | works |
| Battery charge thresholds | **not available**: no `charge_control_{start,end}_threshold` on this model |
| SD card reader | **none usable**: the Bay Trail SD host (ACPI `80860F14`) has `_STA` 0 (disabled by the firmware), so no MMC host appears. A USB card reader works |
| Fingerprint reader | **not present** |
| Mini-HDMI output | connector present; nothing plugged in to test |

The remaining boot messages (`ACPI: \: failed to evaluate _DSM bf0212f2-...`, `thinkpad_acpi: Error probing battery 2`,
`ACPI: battery: extension failed to load: ThinkPad Battery Extension`, `BCTG evaluated but flagged as error`) come from
`thinkpad_acpi` probing features this 2014 model does not have; none needed a kernel parameter or module option.

Bay Trail's old C-state freezes (the reason for `intel_idle.max_cstate=1` in old advice) were fixed in the kernel years
ago; `max_cstate` is left at the default 9. Add the parameter only if the machine freezes when idle.

## The battery on bruce-11e is inoperative

On 2026-11-05 the battery (SMP LNV-45N1751, Li-poly, design 34.04 Wh, full 27.39 Wh, 80% of design) read 0%, a pack
voltage of 3.28 V and "Not charging" while on AC; the EC will not charge a pack drained that far, and Bruce found it
inoperative. Replace the pack (FRU 45N1751 or an equivalent). Until then the laptop runs only on AC, which is how a demo
laptop is used anyway; pulling the power cord turns it off without a clean shutdown.

## What the package sets

* `/etc/tlp.d/20-lenovo-thinkpad-yoga-11e.conf`: governor `performance` on AC, `schedutil` on battery (`intel_cpufreq`
  offers only those two); Wi-Fi power saving off on battery too; webcam and Bluetooth kept out of USB autosuspend.
* `/etc/systemd/logind.conf.d/50-lenovo-thinkpad-yoga-11e.conf`: lid switch ignored on AC power and when docked, suspends
  on battery.
* `/etc/dconf/db/local.d/50-lenovo-thinkpad-yoga-11e`: screen off after 300 s idle; no inactivity suspend on AC; 20 minutes
  on battery.
* `/etc/NetworkManager/conf.d/50-lenovo-thinkpad-yoga-11e.conf`: `wifi.powersave=2` (off) for every connection and
  `autoconnect-retries-default=0` in `[main]` (unlimited reconnect attempts for every connection left at the default).

### Install

```
# build hardware/convertible_power, hardware/convertible_helper and this directory, each in a copy outside the repository
# (dpkg-buildpackage writes to the parent directory)
dpkg-buildpackage -us -uc -b
sudo apt install ./convertible-power_*_all.deb ./convertible-helper_*_all.deb ./lenovo-thinkpad-yoga-11e-support_*_all.deb
```

The lid setting takes effect at the next boot.

## Not verified

Behaviour on battery (the battery does not charge); Wi-Fi with a real association; mini-HDMI projection; screen rotation
and the on-screen keyboard watched on the screen in tablet mode (the sensors and the switch were read over ssh only).

## BIOS

Installed N15ET21W (1.07, 2014-06-13). Not compared with Lenovo's latest for the 20D9 and nothing was flashed. fwupd has no
LVFS entry for this BIOS.

Licence: GPL-3.0-or-later.
