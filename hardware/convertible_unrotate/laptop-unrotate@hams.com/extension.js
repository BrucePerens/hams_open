// SPDX-License-Identifier: GPL-3.0-or-later
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS = 'org.gnome.Mutter.DisplayConfig';
const PATH = '/org/gnome/Mutter/DisplayConfig';
const TAG = 'laptop-unrotate:';

function call(method, args) {
    // Asynchronous: the display service is the shell's own main loop.
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call(BUS, PATH, BUS, method, args, null,
            Gio.DBusCallFlags.NONE, -1, null, (conn, res) => {
                try {
                    resolve(conn.call_finish(res));
                } catch (e) {
                    reject(e);
                }
            });
    });
}

async function setNormal() {
    const state = await call('GetCurrentState', null);
    const [serial, monitors, logicals] = state.recursiveUnpack();
    if (logicals.every(lm => lm[3] === 0))
        return false;
    const modeOf = new Map();
    for (const [[connector], modes] of monitors) {
        for (const [id, , , , , , props] of modes) {
            if (props['is-current'])
                modeOf.set(connector, id);
        }
    }
    const out = logicals.map(([x, y, scale, , primary, mons]) =>
        [x, y, scale, 0, primary, mons.map(([c]) => [c, modeOf.get(c), {}])]);
    await call('ApplyMonitorsConfig',
        new GLib.Variant('(uua(iiduba(ssa{sv}))a{sv})', [serial, 1, out, {}]));
    return true;
}

export default class LaptopUnrotate extends Extension {
    enable() {
        this._seat = global.stage.context.get_backend().get_default_seat();
        this._touchMode = this._seat.touch_mode;
        console.log(`${TAG} enabled, touch-mode=${this._touchMode}`);
        this._id = this._seat.connect('notify::touch-mode', () => {
            const t = this._seat.touch_mode;
            console.log(`${TAG} touch-mode ${this._touchMode} -> ${t}`);
            this._touchMode = t;
            if (!t)
                setNormal().then(r => console.log(`${TAG} reset to normal: ${r}`))
                    .catch(e => console.error(`${TAG} reset failed: ${e}`));
        });
    }

    disable() {
        if (this._id)
            this._seat.disconnect(this._id);
        this._id = 0;
        this._seat = null;
    }
}
