# hp_spectre_x360_16

A Debian package, `hp-spectre-x360-16-support`, for the **HP Spectre x360 2-in-1 Laptop 16-f2xxx**
(13th-gen Core i7-1360P, Intel Raptor Lake) on Debian with GNOME. It is the place for this laptop's settings; today:

* it depends on `convertible-helper` (tablet mode: the on-screen keyboard setting in tablet position, the screen
  reset to normal when leaving tablet mode, see `../convertible_helper`);
* it depends on `convertible-power` (TLP: maximum performance on AC, efficiency on battery, UPower action at 10%,
  reminder to unplug at 80%, see `../convertible_power`) and installs `/etc/tlp.d/20-hp-spectre-x360-16.conf` on this
  model: governors `performance` (AC) and `powersave` (battery), platform profile `performance` (AC) and `cool`
  (battery), PCIe ASPM `powersupersave` on battery.

Found on this laptop (2026-10-28 and -29): the tablet-mode switch and the accelerometer both work (GNOME ignores the
accelerometer outside tablet mode); `hp-bioscfg` exposes only Sure Start, so the BIOS battery-care option cannot be
set from Linux; the fingerprint reader (`06cb:00ff`) is listed by libfprint as unsupported and does not enroll; the
keyboard backlight has no `kbd_backlight` LED device.

Platform profile: `hp-wmi` accepts writes to `/sys/firmware/acpi/platform_profile` but reading it returns `EINVAL`, so
TLP reports the profile as "not available" and ignores `PLATFORM_PROFILE_*`. The package sets it instead from a udev
rule (when the charger changes) and a boot service: `performance` on AC, `cool` on battery.

Not verified: the battery-side settings were not measured (the laptop was on AC), and the choice of `cool` rather
than `quiet` for the battery profile follows what `power-profiles-daemon` does on this firmware; the profile switch
could not be seen in the RAPL limits.

Licence: GPL-3.0-or-later.

## GRUB

The package depends on `grub-large-console` (`../grub_large_console`), which sizes a font so that GRUB's menu is
readable, about 80x24, on this machine's screen. Not yet tried on this machine: it needs a reboot someone can look at.
