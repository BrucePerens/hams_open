# convertible_unrotate

A small GNOME Shell extension (`laptop-unrotate@hams.com`, GNOME 48) for convertible laptops. When
the laptop leaves tablet mode it sets the screen back to normal orientation.

Why: GNOME follows the accelerometer only while the tablet-mode switch is on. On the HP Spectre x360
2-in-1 16-f2xxx the "Intel HID switches" device flickers (1, 0, 1) as the screen is turned and often goes
back to 0 *before* the accelerometer reports `normal`; GNOME then ignores the later `normal` and the
screen stays turned (captured 2026-10-28: switch 0 at 2.5 s, sensor `normal` at 2.6 s, screen left at
180 degrees). A convertible in laptop position is always upright, so the extension resets it when
`Clutter.Seat` `touch-mode` goes false.

It uses mutter's `DisplayConfig` D-Bus API with asynchronous calls (the service lives in the shell's own
main loop, so a blocking call would deadlock it).

Install (per user; log out and in once, GNOME loads new extensions only at login):

```
cp -r laptop-unrotate@hams.com ~/.local/share/gnome-shell/extensions/
gnome-extensions enable laptop-unrotate@hams.com
```

Verified on that one laptop with auto-rotate unlocked
(`gsettings set org.gnome.settings-daemon.peripherals.touchscreen orientation-lock false`).
Licence: GPL-3.0-or-later (a GNOME Shell extension runs inside GPL-2.0-or-later GNOME Shell).
