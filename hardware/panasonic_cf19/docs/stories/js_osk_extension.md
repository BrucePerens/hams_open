# `gnome-shell/cf19-osk@hams.com/extension.js`

The CF-19's keyboard button shows and hides GNOME's on-screen keyboard. Tests: `tests/test_osk_extension.py`.

- `enable()` watches the keyboard's visibility when the driver's tablet switch exists, binds the toggle key, and owns `org.cf19.OSK` on the session bus; a failure is logged, never thrown into the shell. [@ANCHOR: panasonic_cf19:js_osk_enable]
- `disable()` undoes all of that and, in laptop position, switches the keyboard setting back off. [@ANCHOR: panasonic_cf19:js_osk_disable]
- `_readTablet()` says whether the screen is swivelled to tablet position, reading the driver's switch, and says no when it cannot be read. [@ANCHOR: panasonic_cf19:js_osk_read_tablet]
- `_setEnabled()` sets the keyboard setting only when it differs, and makes the shell create or destroy its keyboard at once. [@ANCHOR: panasonic_cf19:js_osk_set_enabled]
- `_onVisibility()` switches the keyboard setting off when the keyboard is hidden in laptop position, so it does not pop up on its own. [@ANCHOR: panasonic_cf19:js_osk_on_visibility]
- `Toggle()` hides a shown keyboard and shows a hidden one. [@ANCHOR: panasonic_cf19:js_osk_toggle]
- `Show()` enables the setting and opens the keyboard on the focused monitor, logging a failure. [@ANCHOR: panasonic_cf19:js_osk_show]
- `Hide()` closes the keyboard. [@ANCHOR: panasonic_cf19:js_osk_hide]
