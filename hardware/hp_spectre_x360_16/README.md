# hp_spectre_x360_16

A Debian package, `hp-spectre-x360-16-support`, for the **HP Spectre x360 2-in-1 Laptop 16-f2xxx**
(13th-gen Core i7-1360P, Intel Raptor Lake) on Debian with GNOME. It is the place for this laptop's settings; today:

* it depends on `convertible-helper` (tablet mode: the on-screen keyboard setting in tablet position, the screen
  reset to normal when leaving tablet mode, see `../convertible_helper`);
* it depends on `convertible-power` (TLP: maximum performance on AC, efficiency on battery, UPower action at 10%,
  reminder to unplug at 80%, see `../convertible_power`) and installs `/etc/tlp.d/20-hp-spectre-x360-16.conf` on this
  model: governors `performance` (AC) and `powersave` (battery), platform profile `performance` (AC) and `cool`
  (battery), PCIe ASPM `powersupersave` on battery.

## Keyboard backlight and camera: findings, nothing done

* **Keyboard backlight.** There is no `kbd_backlight` LED device, no keyboard-backlight method in this firmware's
  ACPI tables (`KBLS` there is the laptop-versus-tablet flag), and the kernel's `hp-wmi` driver (7.1.2) has no
  keyboard-backlight support. The backlight is almost certainly driven by the embedded controller from the keyboard's
  own function key, so GNOME has nothing to show; controlling it from software would need the EC register map.
* **Camera.** An Intel IPU6 (`intel_ipu6_isys`, 8086:a75d-class) with an OmniVision sensor (ACPI `OVTI00AB`) is
  present and the kernel side loads (`/dev/video0` to `video*` "Intel IPU6 ISYS Capture"); libcamera 0.4.0, its
  GStreamer and PipeWire plugins are installed. Debian 13 ships libcamera 0.4.0 and the IPU6 software-ISP path is
  newer; no working capture was obtained. Not investigated further.
* **Fingerprint reader** (`06cb:00ff`): see below.

Found on this laptop (2026-10-28 and -29): the tablet-mode switch and the accelerometer both work (GNOME ignores the
accelerometer outside tablet mode); `hp-bioscfg` exposes only Sure Start, so the BIOS battery-care option cannot be
set from Linux; the fingerprint reader (`06cb:00ff`) is listed by libfprint as unsupported and does not enroll; the
keyboard backlight has no `kbd_backlight` LED device.

Platform profile: `hp-wmi` accepts writes to `/sys/firmware/acpi/platform_profile` but reading it returns `EINVAL`, so
TLP reports the profile as "not available" and ignores `PLATFORM_PROFILE_*`. The package sets it instead from a udev
rule (when the charger changes) and a boot service: `performance` on AC, `cool` on battery.

Runtime power management: TLP's `RUNTIME_PM_DENYLIST` keeps **both** GPUs awake, the integrated GPU (`00:02.0`, which drives the panel) and the discrete Arc A370M (`03:00.0`), since version 1.3.1. Version 1.3.0 let the Arc GPU suspend, which saved 3 to 5 W on battery (2026-10-29: 27.6 W suspended against 31 W awake), but on kernel 7.1.2 the GPU did not always return: from 2026-10-31 to 2026-11-05 the kernel logged `Lost access to MMIO BAR` / `MMIO unreliable` for it eight times, each with an i915 WARNING, and the audio DSP (SOF, `00:1f.3`) hung with IPC timeouts 30 s after one of them until the next reboot. The GuC firmware the kernel recommends (70.53.0; Debian trixie ships 70.36.0) was installed from linux-firmware under `/lib/firmware/updates` on 2026-11-05; once it has run a while, the saving can be measured again with `03:00.0` taken out of the list.

Measured idle power (battery, same brightness): AC-style settings 36.8 to 38.8 W, battery settings 30.8 to 32.4 W (the
Arc GPU still awake), about 16% lower.

Not verified: the battery-side settings were not measured (the laptop was on AC), and the choice of `cool` rather
than `quiet` for the battery profile follows what `power-profiles-daemon` does on this firmware; the profile switch
could not be seen in the RAPL limits.

Licence: GPL-3.0-or-later.

## GRUB

The package depends on `grub-large-console` (`../grub_large_console`), which sizes a font so that GRUB's menu is
readable, about 80x24, on this machine's screen. Not yet tried on this machine: it needs a reboot someone can look at.
