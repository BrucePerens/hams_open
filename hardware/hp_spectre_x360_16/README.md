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

Runtime power management: TLP's `RUNTIME_PM_DENYLIST` keeps the integrated GPU (`00:02.0`, which drives the panel) awake
and lets the discrete Arc A370M (`03:00.0`) suspend. Measured on battery with the same settings (idle, 4K panel, no
other load, 2026-10-29): 30.8 to 32.4 W with the Arc GPU held awake by a driver-level exclusion, **27.6 W** with it
suspended. On kernel 7.1.2 each suspend of the Arc GPU logs one i915 kernel WARNING (`gen2_assert_iir_is_zero`, from
`pm_runtime_work`); the GPU suspends and resumes normally. Forcing runtime PM on both GPUs the first time produced
that warning twice plus an `intel_uncore` unclaimed-write warning, which is why the integrated GPU is left alone. The
earlier belief that the warnings came from the integrated GPU was wrong. If the warning is unwelcome, add `03:00.0` to
`RUNTIME_PM_DENYLIST` in `/etc/tlp.conf`, at a cost of about 3 to 5 W.

Measured idle power (battery, same brightness): AC-style settings 36.8 to 38.8 W, battery settings 30.8 to 32.4 W (the
Arc GPU still awake), about 16% lower.

Not verified: the battery-side settings were not measured (the laptop was on AC), and the choice of `cool` rather
than `quiet` for the battery profile follows what `power-profiles-daemon` does on this firmware; the profile switch
could not be seen in the RAPL limits.

Licence: GPL-3.0-or-later.

## GRUB

The package depends on `grub-large-console` (`../grub_large_console`), which sizes a font so that GRUB's menu is
readable, about 80x24, on this machine's screen. Not yet tried on this machine: it needs a reboot someone can look at.
