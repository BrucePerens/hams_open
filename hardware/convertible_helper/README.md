# convertible_helper

`convertible-helper` is a small user-level service for convertible laptops. It uses no GNOME Shell code,
only the kernel's input events and mutter's public `org.gnome.Mutter.DisplayConfig` D-Bus API, so it does not
break when GNOME Shell changes.

* **Leaving tablet mode resets the screen to normal.** GNOME follows the accelerometer only while the
  tablet-mode switch is on. On the HP Spectre x360 2-in-1 16-f2xxx the switch drops to 0 before the
  accelerometer reports `normal` (captured 2026-10-28: switch 0 at 2.5 s, `normal` at 2.6 s), the later
  `normal` is ignored and the screen stays turned. The helper waits 400 ms (the switch flickers 1, 0, 1 as
  the screen is turned) and then sets the built-in panel upright.
* **Tablet buttons named in a profile.** A `[buttons]` section in a profile file (`rotate = KEY_PROG3`,
  `security = KEY_PROG1`, optional `dmi-product-prefix`) makes the helper act on those keys from the
  `Panasonic Tablet Button Support` device, and only on matching machines. `rotate` turns the built-in panel one
  step clockwise, in any shell state (GNOME's own shortcut is ignored while a dialog is open); `security` opens
  the log-out dialog of GNOME (`gnome-session-quit --logout`), Xfce (`xfce4-session-logout`) or KDE
  (`qdbus6 org.kde.LogoutPrompt`). The Toughbook CF-19 profile is in `hardware/panasonic_cf19/profiles`. Buttons
  not named in a profile (for example the Toughpad FZ-G1's A1 and A2) are left to GNOME's own
  Settings, Keyboard, Custom Shortcuts.

* **Tablets without a tablet-mode switch** (the Panasonic Toughpad FZ-G1 has none; GNOME otherwise shows only its
  emoji key when you touch a text field undocked): a profile file in `/usr/share/convertible-helper/profiles/` or
  `/etc/convertible-helper.d/` names the USB id of the dock keyboard in a `[dock]` section
  (`usb-keyboard = 2047:0318`, optional `dmi-product-prefix = FZ-G1`). While that USB device is absent the tablet
  counts as in tablet mode, with the same effects as the switch; the state is polled once a second from sysfs.

* **Auto-rotate for tablets GNOME does not rotate** (an `[autorotate]` profile section: `iio-name = accel_3d`,
  optional `flip-x`/`flip-y`): while the tablet counts as in tablet mode the helper reads the accelerometer from sysfs
  every 300 ms and sets the built-in display to match. It needs the same orientation twice in a row, keeps the current
  orientation when the tablet lies flat or sits near a boundary, and docking still resets to normal. The sensor's
  reading is gravity's reaction: y negative means the screen is upright, x positive means its right edge is up.

Only the built-in panel is rotated or reset (monitors that report `is-builtin`); external monitors are left
alone. A udev rule (`70-convertible-helper.rules`) gives the user on the seat read access, via `uaccess`, to
switch-type input devices and to the CF-19 button device, and to nothing else.

The service is `convertible-helper.service` (systemd user unit, started with the graphical session; the
package enables it for all users). Log lines go to the user journal:
`journalctl --user -u convertible-helper`.

Needs Python 3 with PyGObject (`python3-gi`). The rotation code uses mutter's DisplayConfig, so rotation needs
GNOME or another mutter-based session; the log-out button also works on Xfce and KDE.

Verified on 2026-10-28 on the HP Spectre x360 2-in-1 16-f2xxx (reset to normal after leaving tablet mode, keyboard
setting in tablet mode; GNOME 48) and on a Toughbook CF-19 (rotation and security buttons). Not verified on other
desktops or GNOME versions.

Licence: GPL-3.0-or-later.
