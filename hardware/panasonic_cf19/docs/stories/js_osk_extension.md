# `gnome-shell/cf19-osk@hams.com/extension.js`

The CF-19's keyboard button shows and hides GNOME's on-screen keyboard. Tests: `tests/test_osk_extension.py`.

## `Cf19OskExtension` [@ANCHOR: panasonic_cf19:js_osk_extension_class]

The file defines one class, `Cf19OskExtension`, the default export of a GNOME Shell extension (GNOME 45 or later module
style). GNOME has no public command to raise or lower its on-screen keyboard, so this is the one piece of the CF-19
support that has to live in the shell; the rotation button, the "security" button and the keyboard setting in tablet mode
belong to `convertible-helper`, which uses no shell code. The keyboard button sends `XF86Launch2`; the extension binds it
as the `toggle-osk` key of the settings schema `org.gnome.shell.extensions.cf19-osk`.

Instance state, all private: `_a11y` (`Gio.Settings` for `org.gnome.desktop.a11y.applications`, whose boolean key
`screen-keyboard-enabled` is the keyboard setting), `_settings` (the extension's own schema), `_haveSwitch` (true when the
driver's tablet switch exists), `_visId` (signal id of the keyboard's `visibility-changed`), `_dbus` (the exported
object) and `_nameId` (the bus-name owner id). Behaviour by position: in tablet position `convertible-helper` has already
enabled the keyboard setting, so the keyboard also pops up when a text field gets focus and the button only shows or
hides it. In laptop position there is no automatic pop-up: the button enables the setting and shows the keyboard, and once
it is hidden again the setting is switched back off. The same three actions are on the session bus as the interface
`org.cf19.OSK` at `/org/cf19/OSK` (`Toggle`, `Show`, `Hide`, no arguments, no return values). The code runs inside the
shell's own process on its main thread; nothing here touches a radio or any transmitter, and the bus interface can only
show or hide the keyboard for the logged-in user's session.

## `enable()` [@ANCHOR: panasonic_cf19:js_osk_enable]

Called by GNOME Shell when the extension is switched on (login, or the user enabling it); no arguments, no return value.
Inside one `try`: creates the two `Gio.Settings` objects; sets `_haveSwitch` from whether the file
`/sys/bus/acpi/devices/MAT0020:00/tablet_mode` (provided by the `panasonic-hbtn` driver) exists; if it does, connects
`_onVisibility` to `Main.keyboard`'s `visibility-changed` signal, and if not, logs that the keyboard setting will be left
alone (driver not loaded) and does not connect; adds the `toggle-osk` key binding with `IGNORE_AUTOREPEAT` in every action
mode, calling `Toggle()`; wraps `this` with `Gio.DBusExportedObject.wrapJSObject` and owns the session-bus name
`org.cf19.OSK`, exporting the object when the connection is ready (and logging when the name is acquired or lost). Any
exception is caught and logged with its stack through `console.error`, never thrown into the shell, so a missing schema
cannot crash the desktop; in that case the extension is left partly set up and `disable()` tolerates it. Not idempotent:
call `disable()` before `enable()` again. Test: `test_enable_binds_the_key_and_owns_the_bus_name`.

## `disable()` [@ANCHOR: panasonic_cf19:js_osk_disable]

Called by GNOME Shell when the extension is switched off; no arguments, no
return value. It removes the `toggle-osk` key binding, disconnects the visibility handler if one was connected, releases
the bus name with `Gio.bus_unown_name`, unexports and drops the D-Bus object, and, only if the tablet switch exists and the
screen is not in tablet position, sets the keyboard setting back to off through `_setEnabled(false)` so a keyboard shown by
the button does not stay enabled once the extension is gone. In tablet position the setting is left on, because
`convertible-helper` owns it there. It finally drops `_a11y` and `_settings`. The steps are each guarded, so calling it
after a failed `enable()` is safe, and calling it twice does nothing harmful. It does not catch exceptions of its own.
Test: `test_disable_undoes_enable_and_switches_the_setting_off_in_laptop_position`.

## `_readTablet()` [@ANCHOR: panasonic_cf19:js_osk_read_tablet]

Returns a boolean: true when the screen is swivelled to tablet position. It reads the file
`/sys/bus/acpi/devices/MAT0020:00/tablet_mode` with `GLib.file_get_contents` and is true only when the read succeeds and the
first byte is ASCII `1` (0x31); `0`, an empty file or any other first byte is false. Any exception (the file does not
exist because the driver is not loaded, or no permission) is caught and gives false, so an unreadable switch is treated as
laptop position, the safe default that makes the keyboard go away when hidden. No arguments, no side effects, cheap (a
tiny sysfs read), called from `Toggle` (for logging), `_onVisibility` and `disable`. It runs synchronously on the shell's
main thread; sysfs reads do not block. Test: `test_read_tablet_reads_the_switch`.

## `_setEnabled(value)` [@ANCHOR: panasonic_cf19:js_osk_set_enabled]

| Argument | Type | Meaning |
|---|---|---|
| `value` | boolean | The wanted state of the GNOME accessibility setting `screen-keyboard-enabled`. |

No return value. It writes the setting with `set_boolean` only when it differs from the current value (so no change signal
fires needlessly), then calls the shell's private `Main.keyboard._syncEnabled?.()` so the shell creates or destroys its
keyboard immediately instead of at the next change notification. The optional call `?.()` means a future GNOME release
without that private method degrades to the delayed behaviour rather than raising; unknown whether newer GNOME versions
still have the method (to-do: check on each GNOME upgrade). Requires `_a11y` to be set, that is, after `enable()` and before
`disable()`; otherwise it throws a `TypeError`. Side effect: a user-visible setting in dconf. Test:
`test_set_enabled_writes_the_setting_only_when_it_changes`.

## `_onVisibility()` [@ANCHOR: panasonic_cf19:js_osk_on_visibility]

The handler of `Main.keyboard`'s `visibility-changed` signal, connected by `enable()` only when the tablet switch exists. No
arguments, no return value. If the screen is not in tablet position (`_readTablet()` false) and the keyboard is no longer
visible (`!Main.keyboard.visible`), it calls `_setEnabled(false)`, so the on-screen keyboard stops popping up on its own
after the user hides it in laptop position. In tablet position it does nothing, since the tablet convention is a keyboard
that appears with every text field. Runs on the shell's main thread each time the keyboard shows or hides; it makes at most
one small sysfs read and one settings write. Test:
`test_on_visibility_switches_the_setting_off_when_the_laptop_keyboard_hides`.

## `Toggle()` [@ANCHOR: panasonic_cf19:js_osk_toggle]

Exported as the D-Bus method `org.cf19.OSK.Toggle` and called by the key binding. No arguments, no return value. It reads
`Main.keyboard.visible`, logs the visibility and the tablet state, and calls `Hide()` when the keyboard is shown or `Show()`
when it is hidden. A key press therefore always flips the state. A repeated press is ignored while held (the binding has
`IGNORE_AUTOREPEAT`). The errors of `Show()` are caught there; `Hide()` does not catch. Test:
`test_toggle_hides_a_shown_keyboard_and_shows_a_hidden_one`.

## `Show()` [@ANCHOR: panasonic_cf19:js_osk_show]

Exported as `org.cf19.OSK.Show`. No arguments, no return value. In one `try` it calls `_setEnabled(true)` (the keyboard cannot
open while the setting is off), picks the monitor (`Main.layoutManager.focusIndex`, or `primaryIndex` when there is none),
logs it and calls `Main.keyboard.open(monitor)`. Any exception is logged with `console.error` and swallowed, so a failing
shell call cannot break the D-Bus method or the key binding. If the keyboard is already shown it just reopens it on that
monitor. In laptop position the setting stays enabled until `_onVisibility` sees the keyboard hidden. Test:
`test_show_enables_the_setting_and_opens_the_keyboard`.

## `Hide()` [@ANCHOR: panasonic_cf19:js_osk_hide]

Exported as `org.cf19.OSK.Hide`. No arguments, no return value. It logs and calls `Main.keyboard.close()`. It does not change
the keyboard setting: in laptop position `_onVisibility` does that once the keyboard has hidden, and in tablet position the
setting stays on. Not wrapped in a `try`, so an exception from the shell would propagate to the D-Bus caller as an error
reply; calling it with the keyboard already closed is expected to be a no-op (not tested; to-do). Test: `test_hide_closes_the_keyboard`.
