# lenovo_yoga_c940_14iil

A Debian package, `lenovo-yoga-c940-14iil-support`, for the **Lenovo Yoga C940-14IIL** (machine type 81Q9, Ice Lake
Core i7, BIOS AUCN59WW, DMI chassis type Convertible) on Debian 13 with GNOME. It follows the pattern of the other laptop
packages in `hardware/`. Settings are applied only when `/sys/class/dmi/id/product_version` begins with
`Lenovo Yoga C940-14IIL`.

It depends on `convertible-helper` (tablet mode, `../convertible_helper`), `convertible-power` (TLP and UPower,
`../convertible_power`; this replaces `power-profiles-daemon`), `grub-large-console` (`../grub_large_console`, readable GRUB
text). On this machine the package was installed with `dpkg --ignore-depends=grub-large-console`, because that package
rewrites GRUB (`update-grub`) and GRUB was off limits during the test; install `grub-large-console` and reboot to see it, and the Debian packages that make the hardware
work: `firmware-iwlwifi` (AX201 Wi-Fi), `firmware-intel-misc` (AX201 Bluetooth `ibt-19-32-4.sfi`), `firmware-sof-signed`
(Smart Sound DSP), `firmware-intel-graphics` (DMC), `alsa-ucm-conf`, `alsa-topology-conf`, `iio-sensor-proxy`, `bluez`. It
recommends `thermald`, `bolt`, `v4l-utils`, `fprintd`, `libpam-fprintd`, `intel-media-va-driver`. Nothing is downloaded.
It installs `/etc/tlp.d/20-lenovo-yoga-c940-14iil.conf` (governors, platform profile `low-power` on battery and
`performance` on AC, both NVMe drives) and `/usr/lib/lenovo-yoga-c940-14iil-support/check-features` (read-only feature check).

A Windows virtual machine for this laptop is planned as a separate binary package from this source directory (a marked
place is in `debian/control`); it is not part of this package.

## Features, verified 2026-10-31 on one machine (kernel 6.12.113, GNOME, 15 GiB RAM)

Status: **verified** = evidence captured; **detected** = device and driver present, a human must try it; **not working**.

| Feature | Status | Evidence / notes |
| --- | --- | --- |
| Camera (Bison 5986:2115, UVC) | verified | `ffmpeg -f v4l2 ... /dev/video0` gave a 1280x720 frame of a real scene (mean luma 126, viewed). `uvcvideo`, PipeWire lists it. Needs no extra package. The `camera_power` switch of ideapad-laptop reads 0. |
| Fingerprint (Synaptics 06cb:00be) | **not working** | libfprint 1.94.9 (Debian 13) lists 06cb:00be under "Known unsupported devices"; `fprintd-list` says "No devices available". The only known driver (synaTudor) is third party and needs the Windows driver, so it is not packaged. |
| Wi-Fi AX201 | verified | `iwlwifi` loaded firmware 77 (`Qu-c0-hr-b0-77.ucode`), associated to the home AP. `iwl-debug-yoyo.bin` "failed to load" lines are optional-file noise. |
| Bluetooth AX201 | verified | `ibt-19-32-4.sfi` loaded, `bluetoothctl list` shows the controller. Pairing a device not tried. |
| Audio device, SOF + ALC298 | verified | `sof-hda-dsp` card; UCM `sof-hda-dsp` present; PipeWire sinks Speaker, 3x HDMI/DP, source Stereo Microphone. Codec subsystem 17aa:3818 matches the kernel's "Lenovo C940" quirk. |
| Speakers | verified | A 1 kHz sine from `speaker-test -D hw:0,0` (2 s, mixer Speaker 63%, unchanged) was recorded by the microphone array: 1 kHz band -23 dB versus -63 dB without tone. Whether the bass speakers are audible is a human check. |
| Microphone array (2 DMIC) | verified | `arecord -D hw:0,6` records; mean -39 dB (room) and tone -23 dB as above. |
| Headphone jack, HDMI/DP audio | detected | Mixer and PipeWire nodes exist; nothing plugged in. |
| Screen brightness | verified | `intel_backlight` (max 496): wrote 200, read back 200, restored. |
| Keyboard backlight | verified | `platform::kbd_backlight` (levels 0..1): wrote 1, read back 1, restored. |
| Accelerometer / auto-rotate | detected | ISH sensor hub, `accel_3d`; `iio-sensor-proxy` reports `AccelerometerOrientation normal`. Whether the screen rotates correctly in tablet mode needs a human. No hwdb mount-matrix entry exists for this model; if the rotation is wrong, add one. |
| Tablet-mode switch | detected | `Lenovo Yoga Tablet Mode Control switch` input device; events not triggered. |
| Touchscreen, pen | detected | `Wacom HID 51E3 Finger` and `Pen` devices (`wacom`, `hid_multitouch`, libwacom 2.14). |
| Touchpad | detected | `SYNA2B31:00 06CB:CD3E Touchpad`. |
| Thunderbolt 3 / USB-C | detected | `thunderbolt` bound, `bolt` recommended. One harmless `uid crc8 mismatch` at boot. Nothing plugged in. |
| HDMI | detected | 3 DP connectors, 0 HDMI cable tested. |
| Hot keys, Fn lock | detected | `Ideapad extra buttons`, `fn_lock` attribute. |
| Power / platform profile | verified | ideapad `platform_profile` readable (low-power, balanced, performance), so TLP sets it. |
| Battery charge limit | **not working** | `conservation_mode` accepts a write but reads back 0; no `charge_control_*_threshold`. TLP thresholds from `convertible-power` have no effect here. Use the BIOS/Windows (Lenovo Vantage) setting. |
| Suspend / hibernate | detected, not tried | `mem_sleep` default `s2idle`, `deep` listed; not suspended (remote machine). |
| SD reader, privacy shutter | not present | This model has neither. |

Observation: the battery reports `energy_full` 6.9 Wh against 60 Wh design (capacity 99%); either a firmware unit quirk or
a worn pack. Not investigated.

## Checklist for Bruce (needs a human)

1. Fingerprint: not supported; nothing to enroll. If a future libfprint supports it: `fprintd-enroll`, then GNOME Settings, Users.
2. Rotate the lid to tablet position: `sudo libinput debug-events` (package `libinput-tools`) shows `SWITCH_TOGGLE tablet-mode`;
   the screen should rotate when the machine is turned and the on-screen keyboard appear.
3. Touch the screen and draw with the pen: `sudo libinput debug-events` shows `TOUCH_DOWN` and `TABLET_TOOL_*`.
4. Listen to both speaker pairs: `speaker-test -D hw:0,0 -c2 -t wav -l 1`; plug in headphones and an HDMI monitor.
5. Fn keys: brightness, volume, mic mute, keyboard backlight, airplane mode.
6. Suspend (`systemctl suspend`) with the lid and power button, check wake, Wi-Fi and camera afterwards.
7. `sudo apt install ./grub-large-console_*.deb` (not yet installed on this machine), reboot once and look at the GRUB menu.

Installing `convertible-power` replaced `power-profiles-daemon` with TLP on this machine (`check-features` printed OK for every
device line; fingerprint WARN as above).

Run `sudo /usr/lib/lenovo-yoga-c940-14iil-support/check-features` any time.

Licence: GPL-3.0-or-later.
