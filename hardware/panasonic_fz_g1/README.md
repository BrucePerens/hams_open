# panasonic_fz_g1

A Debian package, `panasonic-fz-g1-support`, that makes the screen brightness control of a **Panasonic Toughpad
FZ-G1** (first generation) work from the GNOME settings panel on Debian 13.

Everything here was found on one tablet: FZ-G1AABAXRM, BIOS V1.00L14 (April 2013), Core i5-3437U, Intel HD 4000,
Debian 13, kernel 6.12.107, GNOME 48 on Wayland. Other FZ-G1 revisions (Mk2 to Mk4) may differ.

## What is installed

* `/usr/lib/modules-load.d/panasonic-fz-g1.conf` makes sure the `panasonic_laptop` module is loaded. It provides
  the ACPI hotkey device (`MAT0019`) and the Panasonic brightness register.
* On configure, on an FZ-G1 only (DMI product name `FZ-G1*`), the file `/etc/modprobe.d/panasonic-blacklist.conf`
  is moved aside to `...disabled-by-panasonic-fz-g1-support` if it contains only `blacklist panasonic_laptop`.
  That blacklist is a widely copied workaround from older kernels (a second, conflicting backlight device); it is
  not needed on kernel 6.12 and it blocks the driver. The file is moved, never deleted.
* `panasonic-fz-g1-support.service` (a oneshot at boot) re-writes the register
  `/sys/devices/LNXSYSTM:00/LNXSYBUS:00/MAT0019:00/current_brightness` with the value it already holds.

## What was found (2026-10-28)

* The tablet had `/etc/modprobe.d/panasonic-blacklist.conf` from 2022. With the driver blocked, GNOME's
  brightness control (which drives `intel_backlight`) did not seem to change the screen, and the panel was dim.
* `current_brightness` (0 to 255) is the Panasonic register that moves the panel. It read 255 while the panel was
  dim; writing it once (also to a lower value and back to 255) made the panel respond, and afterwards GNOME's
  slider tracked the panel (`intel_backlight` 1498 to 4882 at 30% to 100%). The register stayed at 255 across two
  reboots. **The exact cause is not established**: either the blocked driver (nothing ever wrote the register) or a
  stale state in the embedded controller. The boot service re-asserts the register as a precaution.
* The kernel's own "panasonic" backlight device (registered only with `acpi_backlight=vendor`) writes the AC and
  battery values (`ac_brightness`, `dc_brightness`, range 31 to 101, clamped by the firmware), not
  `current_brightness`. With `acpi_backlight=vendor` the panel did **not** respond to GNOME or to `current_brightness`
  at all, and `intel_backlight` was not registered. That setting was tried and removed; it is not part of the package.
* Both cameras work with no configuration (two UVC devices, 640x480 YUYV and MJPEG up to 1280x720): the
  "USB HD Webcam" (`5986:0529`, `/dev/video0`) is the user-facing camera and the "USB2.0 Camera" (`174f:1a09`,
  `/dev/video2`) is on the back of the unit. The odd-numbered nodes (`video1`, `video3`) are metadata nodes.

* The tablet has no tablet-mode or dock switch (no ACPI event either). Undocking removes the keyboard's USB hub
  (`0424:2514`) and the iKey keyboard behind it (`2047:0318`) within about 150 ms. Without help, touching a text
  field undocked shows only GNOME's emoji key. A profile for `convertible-helper`
  (`/usr/share/convertible-helper/profiles/panasonic-fz-g1.conf`) treats the absence of `2047:0318` as tablet mode:
  undocked, the full on-screen keyboard appears; re-docked, the setting is restored (verified 2026-10-28).

* The bezel buttons A1 and A2 arrive through the ACPI `TBTN` device (`MAT002A`): HINF codes `0x38`/`0x39` (A1) and
  `0x42`/`0x43` (A2). `panasonic-hbtn` 1.4.0 (in the `panasonic-hbtn-dkms` package) reports them as `XF86Launch1`
  and `XF86Launch2`; bind them to anything in GNOME's Settings, Keyboard, Custom Shortcuts (press the button when
  asked for the key). The Windows and folder buttons send the PS/2 Super key, which opens the GNOME overview (the
  folder button adds an unmapped scancode `0x65`). Volume buttons work through `panasonic_laptop`.

* GNOME does not auto-rotate this tablet (no tablet-mode switch and a built-in keyboard, so it never counts as in
  tablet mode). The `[autorotate]` profile makes `convertible-helper` follow the accelerometer (HID sensor hub
  `04b5:0680`, IIO device `accel_3d`, with `flip-x`) while undocked.

## Power

The package depends on `convertible-power` (TLP: maximum performance on AC, efficiency on battery; see
`../convertible_power`) and installs `/etc/tlp.d/20-panasonic-fz-g1.conf` on an FZ-G1: governors `performance` (AC) and
`schedutil` (battery), because kernel 6.12 runs `intel_pstate` in passive mode on this CPU (no HWP, so no
`powersave` governor and no energy preference), and a USB autosuspend denylist for the touch panel, pen, sensor hub
and keyboard dock. No charge limit is available: the firmware has no handler for one (the Panasonic `eco_mode`
register reads as an error, and the ACPI tables have no charge-threshold method).

## Not verified

A cold boot or a battery-removed power cycle (the register's state may be lost), other FZ-G1 revisions, and other
desktops. 

Licence: GPL-3.0-or-later.
