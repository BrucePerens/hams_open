# panasonic_cf19

Debian packages that make a **Panasonic Toughbook CF-19** (a swivel-screen, resistive-touch rugged laptop)
work on a stock **Debian 13 (trixie) GNOME 48** desktop: tablet buttons, the swivel-screen switch, touch
calibration, the on-screen keyboard and the internal speaker.

Everything here was found by inspecting one machine, a CF-19 Mk5 (`CF-19AHUAX1M`, BIOS `V5.00L12`, Core
i5-2520M, 1024x768 LVDS panel), running Debian 13 with kernel 6.12.113, GNOME Shell 48.7 on Wayland and
PipeWire 1.4.2. Other CF-19 revisions and other Toughbooks may differ; see "Not verified" below.

Licence: the driver (`driver/`) is **GPL-2.0-only** because it derives from GPL-2.0-only code. Everything
else is **GPL-3.0-or-later**. See `LICENSING.md` at the repository root and `debian/copyright`.

## Packages

The source package `panasonic-cf19` builds two binary packages.

| Package | What it installs |
|---|---|
| `panasonic-hbtn-dkms` | The `panasonic-hbtn` kernel module as DKMS source (built for each installed kernel, signed by the DKMS key). |
| `panasonic-cf19-support` | The touch calibration udev rule, the GNOME Shell extension `cf19-osk@hams.com` (with its GSettings schema and a default that enables it), and the PipeWire speaker patch. Depends on `panasonic-hbtn-dkms`. |

### Install

```
cd hardware/panasonic_cf19
dpkg-buildpackage -us -uc -b      # run in a copy outside the repository; it writes to the parent directory
sudo apt install ../panasonic-hbtn-dkms_*_all.deb ../panasonic-cf19-support_*_all.deb
```

Build dependencies: `debhelper`, `dh-dkms`. Then **log out and in once** (GNOME loads a new extension
and the touch calibration only at login), and `sudo modprobe panasonic_hbtn` if you do not reboot.

### Remove

`sudo apt remove panasonic-cf19-support panasonic-hbtn-dkms` removes the module, the udev rule, the
extension and its schema, and undoes the speaker diversion.

## What each piece does, and why

### Tablet buttons and the swivel switch (`driver/panasonic-hbtn.c`)

The CF-19 firmware has an ACPI device `TBTN` (`MAT0020`, or `MAT001F` on other firmware). The embedded
controller runs the query methods `_Q95` to `_Q9E`, which push a code into a 32-entry queue and call
`Notify(TBTN, 0x80)`; the method `HINF` returns the next queued code. Nothing in the mainline kernel
binds to it (`panasonic_laptop` handles only `MAT0019`, the hotkey device, which already provides the
volume rocker).

| Firmware codes | Meaning | Key sent |
|---|---|---|
| `0x3a` / `0x3b` | Button 1, on-screen keyboard | `KEY_PROG2` (`XF86Launch2`) |
| `0x38` / `0x39` | Button 2, Enter | `KEY_ENTER` |
| `0x36` / `0x37` | Button 3, rotate | `KEY_ROTATE_DISPLAY` (`XF86RotateWindows`) |
| `0x34` / `0x35` | Button 4, "security" (Ctrl-Alt-Del on Windows) | `KEY_PROG1` (`XF86Launch1`) |
| `0x32` / `0x33` | The screen was swivelled into / out of tablet position | `SW_TABLET_MODE` |

The even code is the press, the odd code the release. Buttons 1 and 4 are sent as a single tap so a
held button does not trigger keyboard auto-repeat. The keys were chosen below code 256 because higher
evdev codes have no X keysym and cannot be bound in GNOME. GNOME's compositor already rotates the
screen on `XF86RotateWindows` (`org.gnome.mutter.keybindings rotate-monitor`).

`/sys/bus/acpi/devices/MAT0020:00/tablet_mode` (read-only, `0` laptop, `1` tablet) is read from the
firmware method `SINF` on every read, for desktop components that cannot see the input switch.

This is a rework of the 2012 `panasonic-hbtn` driver (Heiher, from John Belmonte's `panasonic-laptop.c`)
for kernel 6.12: `devm` resources, `void` remove callback, a `debug` module parameter, draining the
firmware queue, the tablet switch and the sysfs attribute.

### Touch screen (`udev/90-panasonic-cf19-touch.rules`)

The touch panel is a Fujitsu USB device `0430:0502`. It is **single-touch**: one USB interface declared
as a boot mouse, reporting `ABS_X`/`ABS_Y` 0..16383 and button clicks, no multitouch axes and no pressure.
Two fingers on a resistive panel produce one point between them (confirmed by capture). libinput sees it
as a plain absolute pointer, not a touchscreen.

The rule sets `LIBINPUT_CALIBRATION_MATRIX` (a least-squares fit of nine stylus touches: maximum residual
about 5 px) only when the DMI product name begins `CF-19`. The matrix is `1.108201 0.001341 -0.065810
-0.006886 1.124566 -0.038159`.

Known limits: a finger lands about 22 px higher than the stylus on this panel and one calibration cannot
serve both. Because GNOME does not treat the panel as a touchscreen, rotating the display does not rotate
the touch mapping.

### Tablet buttons in GNOME (`gnome-shell/cf19-osk@hams.com`, `schemas/`)

GNOME has no public command to raise or lower its on-screen keyboard; it builds the keyboard only while
the accessibility setting `screen-keyboard-enabled` is on, and then raises it whenever a text field gets
focus. The extension calls the shell's own `Main.keyboard.open()` / `close()` and also exposes them on the
session bus as `org.cf19.OSK` (`Toggle`, `Show`, `Hide`). It registers the two button shortcuts itself,
with key auto-repeat ignored, through its own schema `org.gnome.shell.extensions.cf19-osk` (keys
`toggle-osk`, default `XF86Launch2`, and `security`, default `XF86Launch1`; change them with `gsettings`).

* Button 1 shows and hides the keyboard. The keyboard follows `tablet_mode`: in tablet position the
  setting is on, so the keyboard pops up when a text field is focused; in laptop position there is no
  pop-up, the button turns the setting on and shows the keyboard, and the setting is switched off again
  when the keyboard is hidden. Without the driver (no `tablet_mode` file) the extension leaves the
  setting alone.
* Button 4 ("security") runs `gnome-session-quit --logout`, GNOME's log-out dialog, the nearest Linux
  equivalent of Ctrl-Alt-Del (it logs out by itself after its 60 second countdown unless cancelled).

A GSettings override (`schemas/90_panasonic-cf19.gschema.override`) sets `org.gnome.shell enabled-extensions`
to this extension by default. Note that an override replaces the whole default list, and a user's own
`enabled-extensions` setting takes precedence over it.

### Internal speaker (`pipewire/analog-output-speaker.patch`)

On this machine the Realtek ALC269VB's internal speaker pin plays only while the codec's **Headphone**
mixer element is switched on, but PipeWire's `analog-output-speaker.conf` switches that element off
(`[Element Headphone] switch = off, volume = off`), so the speaker stays silent although everything else
reads correct. The patch lets PipeWire manage that element together with the speaker volume, the
Speaker element being left alone.

`postinst` applies it only when the DMI product name begins `CF-19`, by diverting the file with
`dpkg-divert` and writing the patched copy; `prerm` undoes this. A dpkg trigger re-applies it after a
`pipewire-bin` upgrade. If the patch no longer applies, the package prints a warning and leaves the stock
file in place (the speaker stays silent); re-run `sudo dpkg-reconfigure panasonic-cf19-support` after
adjusting the patch.

## Verified and not verified

Verified on the CF-19 Mk5 named above:

* The packages build with no lintian findings (including pedantic), and install, upgrade, remove, purge and
  reinstall cleanly. Removing them restores PipeWire's speaker file byte for byte and removes the diversion,
  the DKMS module, the udev rule, the extension and the schema.
* All four buttons and both swivel codes arrive from the firmware in real time in both screen positions;
  Enter and rotate work in GNOME; `tablet_mode` reads back correctly.
* The touch calibration matrix reaches the device through the packaged udev rule, and gave about 5 px
  accuracy with the stylus (measured with an identical hand-installed rule).
* The speaker is audible with the patched mixer path (test tones heard), and not without it.
* An earlier form of the extension (D-Bus methods plus ordinary GNOME custom shortcuts) showed and hid the
  keyboard from the button, and the log-out button opened GNOME's log-out dialog.

**Not yet verified, at the time of writing:** the shipped extension itself in a login session, that is, its
own shortcut registration through `org.gnome.shell.extensions.cf19-osk` and the tablet-position-only pop-up
of the keyboard. It builds, installs and its schema and default are active, but nobody has yet pressed the
buttons with it running.

Not verified at all: other CF-19 revisions (a dual-touch capacitive screen, or a different speaker or codec
wiring); other Toughbooks (CF-18, CF-31...); Debian releases other than 13; GNOME other than 48 (the
extension declares `shell-version` 48); Secure Boot (this machine boots in legacy BIOS mode, so DKMS
module signing was not exercised).

## Deliberately not in the package

Machine-specific or personal settings stay local: Wi-Fi credentials, and power behaviour such as keeping
the machine awake with the lid closed on external power. Nothing here changes power management.
