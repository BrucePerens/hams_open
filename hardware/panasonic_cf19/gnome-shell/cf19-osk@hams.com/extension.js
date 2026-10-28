// SPDX-License-Identifier: GPL-3.0-or-later
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {rotateMonitors} from './rotate.js';

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
 * Panasonic Toughbook CF-19 tablet buttons, in GNOME Shell.
 *
 * Keyboard button (XF86Launch2): shows and hides GNOME's on-screen keyboard,
 * and the keyboard follows the swivel position of the screen:
 *  - Tablet position: the shell's keyboard is enabled, so it pops up by itself
 *    when a text field gets focus.  The button shows/hides it.
 *  - Laptop position: no automatic pop-up.  The button enables the keyboard and
 *    shows it; once it is hidden again the setting is switched back off.
 * Without the panasonic-hbtn driver (no tablet_mode file) the extension leaves
 * the keyboard setting alone.
 *
 * Rotation button (XF86RotateWindows): rotates the screen one step, also while a
 * dialog or menu is open (GNOME's own shortcut is ignored in those states, so it
 * is switched off while this extension runs and restored afterwards).
 *
 * Security button (XF86Launch1): the Ctrl-Alt-Del equivalent, GNOME's log-out
 * dialog.
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
                this._tablet = this._readTablet();
                console.log(`${TAG} initial tablet state=${this._tablet}`);
                this._applyMode();

                this._visId = Main.keyboard.connect('visibility-changed', () => this._onVisibility());
                this._pollId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
                    const t = this._readTablet();
                    if (t !== this._tablet) {
                        console.log(`${TAG} tablet state ${this._tablet} -> ${t}`);
                        this._tablet = t;
                        this._applyMode();
                    }
                    return GLib.SOURCE_CONTINUE;
                });
            } else {
                console.log(`${TAG} no tablet_mode switch (driver not loaded): keyboard setting left alone`);
            }

            Main.wm.addKeybinding('toggle-osk', this._settings,
                Meta.KeyBindingFlags.IGNORE_AUTOREPEAT, Shell.ActionMode.ALL,
                () => this.Toggle());
            Main.wm.addKeybinding('security', this._settings,
                Meta.KeyBindingFlags.IGNORE_AUTOREPEAT, Shell.ActionMode.ALL,
                () => this._logout());

            this._mutterKeys = new Gio.Settings({schema_id: 'org.gnome.mutter.keybindings'});
            this._savedRotate = this._mutterKeys.get_value('rotate-monitor');
            if (this._savedRotate.get_strv().length === 0)   // left empty by an earlier crash
                this._savedRotate = new GLib.Variant('as', ['XF86RotateWindows']);
            this._mutterKeys.set_strv('rotate-monitor', []);
            Main.wm.addKeybinding('rotate', this._settings,
                Meta.KeyBindingFlags.IGNORE_AUTOREPEAT, Shell.ActionMode.ALL,
                () => this._rotate());

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
        Main.wm.removeKeybinding('security');
        Main.wm.removeKeybinding('rotate');
        if (this._mutterKeys && this._savedRotate)
            this._mutterKeys.set_value('rotate-monitor', this._savedRotate);
        this._mutterKeys = null;
        this._savedRotate = null;
        if (this._pollId) {
            GLib.source_remove(this._pollId);
            this._pollId = 0;
        }
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
        if (this._haveSwitch && this._a11y)
            this._setEnabled(false);
        this._a11y = null;
        this._settings = null;
    }

    _readTablet() {
        try {
            const [ok, bytes] = GLib.file_get_contents(TABLET_MODE_PATH);
            return ok && bytes.length > 0 && bytes[0] === 0x31;   // '1'
        } catch (e) {
            return false;
        }
    }

    _setEnabled(value) {
        if (this._a11y.get_boolean(KEY) !== value)
            this._a11y.set_boolean(KEY, value);
        // Make the shell create/destroy its keyboard right now.
        Main.keyboard._syncEnabled?.();
    }

    _applyMode() {
        this._setEnabled(this._tablet);
    }

    _onVisibility() {
        // Laptop position: once the keyboard is hidden, stop it popping up on its own.
        if (!this._tablet && !Main.keyboard.visible)
            this._setEnabled(false);
    }

    _rotate() {
        rotateMonitors(m => console.log(`${TAG} ${m}`))
            .then(t => console.log(`${TAG} rotate button: primary transform now ${t}`))
            .catch(e => console.error(`${TAG} rotate failed: ${e}`));
    }

    _logout() {
        console.log(`${TAG} security button: opening the log-out dialog`);
        Gio.Subprocess.new(['gnome-session-quit', '--logout'], Gio.SubprocessFlags.NONE);
    }

    Toggle() {
        const vis = !!Main.keyboard.visible;
        console.log(`${TAG} Toggle() visible=${vis} tablet=${this._tablet}`);
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
