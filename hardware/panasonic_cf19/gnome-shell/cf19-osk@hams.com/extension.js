// SPDX-License-Identifier: GPL-3.0-or-later
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const IFACE = `<node>
  <interface name="org.cf19.OSK">
    <method name="Toggle"/>
    <method name="Show"/>
    <method name="Hide"/>
  </interface>
</node>`;

const TAG = 'cf19-osk:';
const KEY = 'screen-keyboard-enabled';
// Provided by the panasonic-hbtn driver: 1 = screen swivelled to tablet position.
const TABLET_MODE_PATH = '/sys/bus/acpi/devices/MAT0020:00/tablet_mode';

/*
 * Panasonic Toughbook CF-19 keyboard button, in GNOME Shell.
 *
 * GNOME has no public command to raise or lower its on-screen keyboard, so this
 * is the one piece of the CF-19 support that has to live in the shell; the
 * rotation button, the "security" button and the keyboard setting in tablet
 * mode are handled by convertible-helper, which uses no shell code.
 *
 * Keyboard button (XF86Launch2): shows and hides GNOME's on-screen keyboard.
 *  - Tablet position: convertible-helper has enabled the keyboard setting, so it
 *    also pops up by itself when a text field gets focus.  The button
 *    shows/hides it.
 *  - Laptop position: no automatic pop-up.  The button enables the keyboard
 *    and shows it; once it is hidden again the setting is switched back off.
 *
 * The same actions are available on the session bus as org.cf19.OSK
 * (Toggle, Show, Hide).
 */
export default class Cf19OskExtension extends Extension {
    enable() {
        console.log(`${TAG} enable() start`);
        try {
            this._a11y = new Gio.Settings({schema_id: 'org.gnome.desktop.a11y.applications'});
            this._settings = new Gio.Settings({schema_id: 'org.gnome.shell.extensions.cf19-osk'});
            this._haveSwitch = GLib.file_test(TABLET_MODE_PATH, GLib.FileTest.EXISTS);
            if (this._haveSwitch) {
                this._visId = Main.keyboard.connect('visibility-changed', () => this._onVisibility());
            } else {
                console.log(`${TAG} no tablet_mode switch (driver not loaded): keyboard setting left alone`);
            }

            Main.wm.addKeybinding('toggle-osk', this._settings,
                Meta.KeyBindingFlags.IGNORE_AUTOREPEAT, Shell.ActionMode.ALL,
                () => this.Toggle());

            this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
            this._nameId = Gio.bus_own_name(
                Gio.BusType.SESSION, 'org.cf19.OSK', Gio.BusNameOwnerFlags.NONE,
                connection => {
                    this._dbus.export(connection, '/org/cf19/OSK');
                    console.log(`${TAG} object exported on bus`);
                },
                () => console.log(`${TAG} name org.cf19.OSK acquired`),
                () => console.log(`${TAG} name org.cf19.OSK LOST`));
            console.log(`${TAG} enable() done`);
        } catch (e) {
            console.error(`${TAG} enable() failed: ${e}\n${e.stack}`);
        }
    }

    disable() {
        console.log(`${TAG} disable()`);
        Main.wm.removeKeybinding('toggle-osk');
        if (this._visId) {
            Main.keyboard.disconnect(this._visId);
            this._visId = 0;
        }
        if (this._nameId) {
            Gio.bus_unown_name(this._nameId);
            this._nameId = 0;
        }
        this._dbus?.unexport();
        this._dbus = null;
        if (this._haveSwitch && this._a11y && !this._readTablet())
            this._setEnabled(false);
        this._a11y = null;
        this._settings = null;
    }

    _readTablet() {
        try {
            const [ok, bytes] = GLib.file_get_contents(TABLET_MODE_PATH);
            return ok && bytes.length > 0 && bytes[0] === 0x31;   // '1'
        } catch (_e) {
            return false;
        }
    }

    _setEnabled(value) {
        if (this._a11y.get_boolean(KEY) !== value)
            this._a11y.set_boolean(KEY, value);
        // Make the shell create/destroy its keyboard right now.
        Main.keyboard._syncEnabled?.();
    }

    _onVisibility() {
        // Laptop position: once the keyboard is hidden, stop it popping up on its own.
        if (!this._readTablet() && !Main.keyboard.visible)
            this._setEnabled(false);
    }

    Toggle() {
        const vis = !!Main.keyboard.visible;
        console.log(`${TAG} Toggle() visible=${vis} tablet=${this._readTablet()}`);
        if (vis)
            this.Hide();
        else
            this.Show();
    }

    Show() {
        try {
            this._setEnabled(true);
            const monitor = Main.layoutManager.focusIndex ?? Main.layoutManager.primaryIndex;
            console.log(`${TAG} Show() monitor=${monitor} keyboardActor=${!!Main.keyboard.keyboardActor}`);
            Main.keyboard.open(monitor);
        } catch (e) {
            console.error(`${TAG} Show() failed: ${e}`);
        }
    }

    Hide() {
        console.log(`${TAG} Hide()`);
        Main.keyboard.close();
    }
}
