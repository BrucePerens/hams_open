// SPDX-License-Identifier: GPL-3.0-or-later
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const BUS = 'org.gnome.Mutter.DisplayConfig';
const PATH = '/org/gnome/Mutter/DisplayConfig';

function call(method, args) {
    return new Promise((resolve, reject) => {
        // Asynchronous on purpose: inside gnome-shell the service answering this
        // call is the shell's own main loop, so a blocking call would deadlock.
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

// Rotate every logical monitor one step clockwise (0 -> 90 -> 180 -> 270 -> 0)
// through mutter's DisplayConfig D-Bus API, the same operation as GNOME's own
// rotate-monitor shortcut but callable in any shell state.  Resolves to the new
// transform of the primary monitor.
export async function rotateMonitors(log = () => {}) {
    const state = await call('GetCurrentState', null);
    const [serial, monitors, logicals] = state.recursiveUnpack();

    // connector -> id of the mode currently in use
    const modeOf = new Map();
    for (const [[connector], modes] of monitors) {
        for (const [id, , , , , , props] of modes) {
            if (props['is-current'])
                modeOf.set(connector, id);
        }
    }

    let primaryTransform = null;
    const out = [];
    for (const [x, y, scale, transform, primary, mons] of logicals) {
        const next = (transform + 1) % 4;
        if (primary)
            primaryTransform = next;
        out.push([x, y, scale, next, primary,
            mons.map(([connector]) => [connector, modeOf.get(connector), {}])]);
    }
    log(`rotate: serial ${serial}, ${out.length} logical monitor(s)`);

    await call('ApplyMonitorsConfig',
        new GLib.Variant('(uua(iiduba(ssa{sv}))a{sv})', [serial, 1, out, {}]));
    return primaryTransform;
}
