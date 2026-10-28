# convertible_helper

`convertible-helper` is a small user-level service for convertible laptops. It uses no GNOME Shell code,
only the kernel's input events and mutter's public `org.gnome.Mutter.DisplayConfig` D-Bus API, so it does not
break when GNOME Shell changes.

* **Leaving tablet mode resets the screen to normal.** GNOME follows the accelerometer only while the
  tablet-mode switch is on. On the HP Spectre x360 2-in-1 16-f2xxx the switch drops to 0 before the
  accelerometer reports `normal` (captured 2026-10-28: switch 0 at 2.5 s, `normal` at 2.6 s), the later
  `normal` is ignored and the screen stays turned. The helper waits 400 ms (the switch flickers 1, 0, 1 as
  the screen is turned) and then sets the built-in panel upright.
* **Toughbook CF-19 rotation button** (`KEY_PROG3`, sent by `panasonic-hbtn` 1.3.0 and later): rotates the
  built-in panel one step clockwise, in any shell state (GNOME's own shortcut is ignored while a dialog is
  open).
* **Toughbook CF-19 "security" button** (`KEY_PROG1`): opens the log-out dialog of GNOME
  (`gnome-session-quit --logout`), Xfce (`xfce4-session-logout`) or KDE (`qdbus6 org.kde.LogoutPrompt`).

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
