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
| `panasonic-cf19-support` | The touch relay service and its udev rule, the GNOME Shell extension `cf19-osk@hams.com` (keyboard button only) (with its GSettings schema and a default that enables it), and the PipeWire speaker patch. Depends on `panasonic-hbtn-dkms` and on `convertible-helper`. |

### Install

```
# build hardware/convertible_helper and hardware/panasonic_cf19 the same way (each in a copy outside the repository;
# dpkg-buildpackage writes to the parent directory)
cd hardware/panasonic_cf19
dpkg-buildpackage -us -uc -b
sudo apt install ./convertible-helper_*_all.deb ./panasonic-hbtn-dkms_*_all.deb ./panasonic-cf19-support_*_all.deb
```

Build dependencies: `debhelper`, `dh-dkms`. Then **log out and in once** (GNOME loads a new extension
and the touch relay at login or boot), and `sudo modprobe panasonic_hbtn` if you do not reboot. Reboot (or re-plug the touch panel) once so GNOME stops using the physical panel.

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
| `0x36` / `0x37` | Button 3, rotate | `KEY_PROG3` (`XF86Launch3`) |
| `0x34` / `0x35` | Button 4, "security" (Ctrl-Alt-Del on Windows) | `KEY_PROG1` (`XF86Launch1`) |
| `0x32` / `0x33` | The screen was swivelled into / out of tablet position | `SW_TABLET_MODE` |

The even code is the press, the odd code the release. Buttons 1 and 4 are sent as a single tap so a
held button does not trigger keyboard auto-repeat. The keys were chosen below code 256 because higher
evdev codes have no X keysym and cannot be bound in GNOME. The rotate button deliberately does not send
`KEY_ROTATE_DISPLAY`: GNOME's own shortcut for that key is ignored while a dialog or menu is open, so
`convertible-helper` (a separate package, see `../convertible_helper`) reads the key events itself and rotates
through mutter's `DisplayConfig` D-Bus API; `KEY_PROG3` is a key GNOME does not bind, so nothing else reacts.
The helper also handles button 4 and sets the keyboard setting in tablet mode.

`/sys/bus/acpi/devices/MAT0020:00/tablet_mode` (read-only, `0` laptop, `1` tablet) is read from the
firmware method `SINF` on every read, for desktop components that cannot see the input switch.

This is a rework of the 2012 `panasonic-hbtn` driver (Heiher, from John Belmonte's `panasonic-laptop.c`)
for kernel 6.12: `devm` resources, `void` remove callback, a `debug` module parameter, draining the
firmware queue, the tablet switch and the sysfs attribute.

### Touch screen (`touch-relay/`, `udev/90-panasonic-cf19-touch.rules`)

The touch panel is a Fujitsu USB device `0430:0502`. It is **single-touch**: one USB interface declared
as a boot mouse, reporting `ABS_X`/`ABS_Y` 0..16383 and button clicks, no touch capability, no pressure and
no axis resolution. Two fingers on a resistive panel produce one point between them (confirmed by capture).
libinput therefore treats it as a plain absolute pointer, and GNOME does not rotate a pointer's mapping when
the display is rotated. Giving the panel a touch capability and a size through udev and hwdb was tried
(`BTN_TOUCH` remap, `ID_INPUT_TOUCHSCREEN`, `EVDEV_ABS_*`): libinput still reported "pointer touch" and GNOME
still did not rotate it.

So on a CF-19 the udev rule tells libinput to ignore the physical panel and starts
`panasonic-cf19-touch@eventN.service`, which runs `cf19-touch-relay`: it reads the panel, applies the
calibration and re-emits the result through uinput as a direct-input, touch-only device. GNOME treats that
as a touchscreen and rotates it with the display. The calibration matrix is the one the previous libinput
rule used (a least-squares fit of nine stylus touches, maximum residual about 5 px on 1024x768):
`1.108201 0.001341 -0.065810 -0.006886 1.124566 -0.038159`, now applied by the relay. The service stops when
the panel disappears and starts again when it returns. It needs `python3` and runs as root (it opens
`/dev/uinput`).

Because the panel is now a touchscreen, taps are touch events (long-press is right-click in GNOME, a drag
scrolls). A finger lands about 22 px higher than the stylus on this panel and one calibration cannot serve
both. After installing, reboot or re-plug the panel so GNOME stops using the physical device.

### Tablet buttons in GNOME (`gnome-shell/cf19-osk@hams.com`, `schemas/`)

GNOME has no public command to raise or lower its on-screen keyboard; it builds the keyboard only while
the accessibility setting `screen-keyboard-enabled` is on, and then raises it whenever a text field gets
focus. The extension is the one piece that has to live in the shell: it calls the shell's own
`Main.keyboard.open()` / `close()` for the keyboard button and exposes them on the session bus as
`org.cf19.OSK` (`Toggle`, `Show`, `Hide`). It registers the button shortcut itself, with key auto-repeat
ignored, through its own schema `org.gnome.shell.extensions.cf19-osk` (key `toggle-osk`, default
`XF86Launch2`; change it with `gsettings`).

* Button 1 shows and hides the keyboard. In tablet position `convertible-helper` has turned the setting on,
  so the keyboard also pops up when a text field is focused; in laptop position there is no pop-up, the
  button turns the setting on and shows the keyboard, and the setting is switched off again when the
  keyboard is hidden. Without the driver (no `tablet_mode` file) the extension leaves the setting alone.

The other buttons are handled by `convertible-helper`, which uses no shell code: button 3 (rotate) rotates
the built-in display one step clockwise, in any shell state; button 4 ("security") opens the desktop's log-out
dialog (`gnome-session-quit --logout` on GNOME), the nearest Linux equivalent of Ctrl-Alt-Del; and leaving
tablet mode puts the screen back to normal.

A GSettings override (`schemas/90_panasonic-cf19.gschema.override`) sets `org.gnome.shell enabled-extensions`
to this extension by default. Note that an override replaces the whole default list, and a user's own
`enabled-extensions` setting takes precedence over it.

### Internal speaker (`pipewire/analog-output-speaker.diff`)

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
* The touch relay (built into the package, started by the udev rule) presents a touch-only device; with it,
  taps stay accurate with the stylus (about 5 px) and GNOME rotates the touch mapping together with the display
  (tested by tapping the corners in both orientations, using a hand-installed copy of the same relay; the
  packaged service is the same code and runs and hides the physical panel).
* With `convertible-helper` (package 1.0.0) and this package at 1.3.0, tested on 2026-10-28 after a fresh login:
  the rotation button rotates once per press in any shell state, including while GNOME's log-out dialog is
  open (GNOME's own shortcut ignores the key in that state); the "security" button opens the log-out dialog
  once; the keyboard button shows and hides the keyboard in laptop position with no automatic pop-up; in
  tablet position tapping a text field raises the keyboard; swivelling back to laptop position while the
  screen is rotated returns it upright.
* The panel gives no way to tell finger from stylus: a raw capture of its HID reports (nine alternating
  presses) shows only a button bit and the two coordinates, and the unused high bits of each 28-bit
  coordinate field are always zero. There is no pressure, tilt or tool information.
* The speaker is audible with the patched mixer path (test tones heard), and not without it.
* The shipped extension, in a fresh GNOME login session on 2026-10-28: it loads active from the package with
  its own schema keys and the session-bus name. In laptop position, tapping a text field does not raise the
  keyboard; button 1 shows and hides it; button 4 opens the log-out dialog. In tablet position, tapping a
  text field raises the keyboard by itself.

Not verified at all: other CF-19 revisions (a dual-touch capacitive screen, or a different speaker or codec
wiring); other Toughbooks (CF-18, CF-31...); Debian releases other than 13; GNOME other than 48 (the
extension declares `shell-version` 48); Secure Boot (this machine boots in legacy BIOS mode, so DKMS
module signing was not exercised).

## Deliberately not in the package

Machine-specific or personal settings stay local: Wi-Fi credentials, and power behaviour such as keeping
the machine awake with the lid closed on external power. Nothing here changes power management.
