// SPDX-License-Identifier: GPL-3.0-or-later
// Runs one named test of the CF-19 on-screen-keyboard GNOME Shell extension:
//   node test_osk_extension_harness.cjs <path to extension.js> <test name>
// extension.js is an ES module that imports GNOME's own modules (gi://..., resource:///...), which exist only inside the shell, so the
// import lines are replaced with stand-ins that record what the extension asks of them. Exit status 0 means the test passed.
"use strict";
const assert = require("assert");
const fs = require("fs");
const vm = require("vm");

const TABLET_MODE_PATH = "/sys/bus/acpi/devices/MAT0020:00/tablet_mode";

/** Loads the extension with the given stand-in state; returns an instance and the record of what it did. */
function make({ haveSwitch = true, tablet = false, keyboardVisible = false, readFails = false, throwOnSettings = false } = {}) {
    const seen = { log: [], errors: [], keybindings: [], removed: [], connected: [], disconnected: [], owned: [], unowned: [], exported: 0, unexported: 0, opened: [], closed: 0, synced: 0, a11y: {}, settingsSchemas: [] };
    const keyboard = {
        visible: keyboardVisible,
        connect: (name, handler) => { seen.connected.push([name, handler]); return 41; },
        disconnect: (id) => seen.disconnected.push(id),
        open: (monitor) => seen.opened.push(monitor),
        close: () => { seen.closed += 1; },
        _syncEnabled: () => { seen.synced += 1; },
    };
    const modules = {
        "gi://Gio": {
            Settings: class {
                constructor({ schema_id }) {
                    if (throwOnSettings) throw new Error("no schema");
                    this.schema = schema_id;
                    seen.settingsSchemas.push(schema_id);
                }
                get_boolean(key) { return !!seen.a11y[key]; }
                set_boolean(key, value) { seen.a11y[key] = value; }
            },
            DBusExportedObject: { wrapJSObject: () => ({ export: () => { seen.exported += 1; }, unexport: () => { seen.unexported += 1; } }) },
            BusType: { SESSION: "session" },
            BusNameOwnerFlags: { NONE: 0 },
            bus_own_name: (...args) => { seen.owned.push(args); return 77; },
            bus_unown_name: (id) => seen.unowned.push(id),
        },
        "gi://GLib": {
            FileTest: { EXISTS: 1 },
            file_test: () => haveSwitch,
            file_get_contents: (path) => {
                assert.strictEqual(path, TABLET_MODE_PATH);
                if (readFails) throw new Error("unreadable");
                return [true, Uint8Array.from([tablet ? 0x31 : 0x30, 0x0a])];
            },
        },
        "gi://Meta": { KeyBindingFlags: { IGNORE_AUTOREPEAT: 4 } },
        "gi://Shell": { ActionMode: { ALL: 255 } },
        "resource:///org/gnome/shell/extensions/extension.js": { Extension: class {} },
        "resource:///org/gnome/shell/ui/main.js": {
            keyboard,
            wm: { addKeybinding: (...args) => seen.keybindings.push(args), removeKeybinding: (name) => seen.removed.push(name) },
            layoutManager: { focusIndex: 2, primaryIndex: 0 },
        },
    };
    let source = fs.readFileSync(process.argv[2], "utf8");
    source = source.replace(/^import (\* as )?(\{[^}]*\}|\w+) from '([^']+)';$/gm, (_m, _star, name, module) => `const ${name} = __modules[${JSON.stringify(module)}];`);
    source = source.replace("export default class Cf19OskExtension", "class Cf19OskExtension") + "\nCf19OskExtension;";
    const context = vm.createContext({ __modules: modules, console: { log: (...a) => seen.log.push(a.join(" ")), error: (...a) => seen.errors.push(a.join(" ")) }, Uint8Array });
    const Extension = vm.runInContext(source, context);
    return { extension: new Extension(), seen, keyboard };
}

const tests = {
    enable() {
        const { extension, seen, keyboard } = make({ haveSwitch: true });
        extension.enable();
        assert.deepStrictEqual(seen.settingsSchemas, ["org.gnome.desktop.a11y.applications", "org.gnome.shell.extensions.cf19-osk"]);
        assert.strictEqual(seen.connected[0][0], "visibility-changed");
        assert.strictEqual(seen.keybindings.length, 1);
        assert.strictEqual(seen.keybindings[0][0], "toggle-osk");
        assert.strictEqual(seen.owned[0][1], "org.cf19.OSK");
        // The keybinding runs Toggle; the bus connection exports the object at its path.
        keyboard.visible = false;
        seen.keybindings[0][4]();
        assert.deepStrictEqual(seen.opened, [2]);
        seen.owned[0][3]({});
        assert.strictEqual(seen.exported, 1);
        // Without the driver's tablet switch the keyboard setting is left alone.
        const bare = make({ haveSwitch: false });
        bare.extension.enable();
        assert.strictEqual(bare.seen.connected.length, 0);
        assert.strictEqual(bare.seen.keybindings.length, 1);
        // A failure while enabling is logged, not thrown into the shell.
        const failing = make({ throwOnSettings: true });
        failing.extension.enable();
        assert.strictEqual(failing.seen.errors.length, 1);
    },

    disable() {
        const { extension, seen } = make({ haveSwitch: true, tablet: false });
        extension.enable();
        seen.a11y["screen-keyboard-enabled"] = true;
        extension.disable();
        assert.deepStrictEqual(seen.removed, ["toggle-osk"]);
        assert.deepStrictEqual(seen.disconnected, [41]);
        assert.deepStrictEqual(seen.unowned, [77]);
        assert.strictEqual(seen.unexported, 1);
        // In laptop position the keyboard setting is switched back off.
        assert.strictEqual(seen.a11y["screen-keyboard-enabled"], false);
        assert.strictEqual(extension._a11y, null);
        // In tablet position it is left on.
        const tablet = make({ haveSwitch: true, tablet: true });
        tablet.extension.enable();
        tablet.seen.a11y["screen-keyboard-enabled"] = true;
        tablet.extension.disable();
        assert.strictEqual(tablet.seen.a11y["screen-keyboard-enabled"], true);
        // Disabling an extension that never enabled does not throw.
        make().extension.disable();
    },

    read_tablet() {
        assert.strictEqual(make({ tablet: true }).extension._readTablet(), true);
        assert.strictEqual(make({ tablet: false }).extension._readTablet(), false);
        assert.strictEqual(make({ readFails: true }).extension._readTablet(), false);
    },

    set_enabled() {
        const { extension, seen } = make();
        extension._a11y = new (class { constructor() { this.v = false; } get_boolean() { return this.v; } set_boolean(_k, v) { this.v = v; seen.sets = (seen.sets || 0) + 1; } })();
        extension._setEnabled(true);
        extension._setEnabled(true);
        assert.strictEqual(seen.sets, 1);
        assert.strictEqual(seen.synced, 2);
        extension._setEnabled(false);
        assert.strictEqual(seen.sets, 2);
    },

    on_visibility() {
        // Laptop position and keyboard hidden: stop it popping up on its own.
        const hidden = make({ tablet: false, keyboardVisible: false });
        hidden.extension._a11y = { get_boolean: () => true, set_boolean: (_k, v) => { hidden.seen.a11y.set = v; } };
        hidden.extension._onVisibility();
        assert.strictEqual(hidden.seen.a11y.set, false);
        const shown = make({ tablet: false, keyboardVisible: true });
        shown.extension._a11y = { get_boolean: () => true, set_boolean: (_k, v) => { shown.seen.a11y.set = v; } };
        shown.extension._onVisibility();
        assert.strictEqual(shown.seen.a11y.set, undefined);
        const tablet = make({ tablet: true, keyboardVisible: false });
        tablet.extension._a11y = { get_boolean: () => true, set_boolean: (_k, v) => { tablet.seen.a11y.set = v; } };
        tablet.extension._onVisibility();
        assert.strictEqual(tablet.seen.a11y.set, undefined);
    },

    toggle() {
        const open = make({ keyboardVisible: true });
        open.extension._a11y = { get_boolean: () => true, set_boolean() {} };
        open.extension.Toggle();
        assert.strictEqual(open.seen.closed, 1);
        assert.strictEqual(open.seen.opened.length, 0);
        const closed = make({ keyboardVisible: false });
        closed.extension._a11y = { get_boolean: () => false, set_boolean() {} };
        closed.extension.Toggle();
        assert.deepStrictEqual(closed.seen.opened, [2]);
    },

    show() {
        const { extension, seen } = make();
        extension._a11y = { get_boolean: () => false, set_boolean: (_k, v) => { seen.a11y.value = v; } };
        extension.Show();
        assert.strictEqual(seen.a11y.value, true);
        assert.deepStrictEqual(seen.opened, [2]);
        // A failure is logged, not thrown into the shell.
        const failing = make();
        failing.extension._a11y = null;
        failing.extension.Show();
        assert.strictEqual(failing.seen.errors.length, 1);
    },

    hide() {
        const { extension, seen } = make();
        extension.Hide();
        assert.strictEqual(seen.closed, 1);
    },
};

const run = tests[process.argv[3]];
if (!run) {
    console.error(`no test named ${process.argv[3]}`);
    process.exit(2);
}
try {
    run();
    process.exit(0);
} catch (error) {
    console.error(error && error.stack ? error.stack : error);
    process.exit(1);
}
