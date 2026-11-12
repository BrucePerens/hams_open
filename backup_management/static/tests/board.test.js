/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// The backup board component (BackupBoard) against stand-ins for the orm and website services.
import { afterEach, describe, expect, test } from "@odoo/hoot";
import { animationFrame } from "@odoo/hoot-dom";
import { App } from "@odoo/owl";
import { getTemplate } from "@web/core/templates";
import { BackupBoard } from "@backup_management/components/board/board";

const CONFIGS = [{ id: 3, name: "main", engine: "pgbackrest", target_path: "/backups", is_stale: false, latest_job: null, latest_snapshot: null }];

/** Records every orm call and answers get_board_data with CONFIGS. */
function stubOrm(calls) {
    return {
        call: async (model, method, args, kwargs) => {
            calls.push([model, method, args, kwargs]);
            return method === "get_board_data" ? CONFIGS : true;
        },
    };
}

/** A board with the services and state `setup()` leaves, without mounting it. */
function bareBoard(calls, website = {}) {
    return Object.assign(Object.create(BackupBoard.prototype), {
        orm: stubOrm(calls),
        website,
        state: { configs: [], isLoading: true },
    });
}

/** Mounts a component in a plain Owl app with only the given services: the full web client environment of mountWithCleanup starts services that call a server this suite lacks. */
async function mountPlain(Component, services = {}, props = {}) {
    const target = document.createElement("div");
    document.body.appendChild(target);
    const app = new App(Component, { env: { services }, props, getTemplate, translateFn: (text) => text });
    cleanups.push(() => { app.destroy(); target.remove(); });
    try {
        return await app.mount(target);
    } catch (error) {
        throw new Error("mount failed: " + String((error.cause && error.cause.stack) || error));
    }
}
const cleanups = [];
afterEach(() => { while (cleanups.length) { cleanups.pop()(); } });

describe("backup_management_board", () => {
    describe.current.tags("backup_management_board");

    test("setup loads the configurations when the board is mounted", async () => {
        // Tests [@ANCHOR: backup_management_js_board_backup_board]
        // Tests [@ANCHOR: backup_management:js_board_setup]
        const calls = [];
        const board = await mountPlain(BackupBoard, { orm: stubOrm(calls), website: { currentWebsite: { id: 2 } } });
        await animationFrame();
        expect(calls).toEqual([["backup.config", "get_board_data", [], { context: { website_id: 2 } }]]);
        expect(board.state.configs).toEqual(CONFIGS);
        expect(board.state.isLoading).toBe(false);
        expect(document.body.textContent).toInclude("main");
    });

    test("fetchData asks for the current website's configurations, or all of them when there is no website", async () => {
        // Tests [@ANCHOR: backup_management:js_board_fetch_data]
        const calls = [];
        const withSite = bareBoard(calls, { currentWebsite: { id: 7 } });
        await withSite.fetchData();
        const withoutSite = bareBoard(calls, { currentWebsite: null });
        await withoutSite.fetchData();
        expect(calls.map((call) => call[3])).toEqual([{ context: { website_id: 7 } }, { context: {} }]);
        expect(withSite.state.configs).toEqual(CONFIGS);
        expect(withSite.state.isLoading).toBe(false);
    });

    test("triggerBackup starts a backup for the configuration and reloads the board", async () => {
        // Tests [@ANCHOR: backup_management:js_board_trigger_backup]
        const calls = [];
        await bareBoard(calls).triggerBackup(3);
        expect(calls.map((call) => call[1])).toEqual(["action_trigger_backup", "get_board_data"]);
        expect(calls[0][2]).toEqual([3]);
    });

    test("syncSnapshots syncs the configuration's snapshots and reloads the board", async () => {
        // Tests [@ANCHOR: backup_management:js_board_sync_snapshots]
        const calls = [];
        await bareBoard(calls).syncSnapshots(3);
        expect(calls.map((call) => call[1])).toEqual(["action_sync_snapshots", "get_board_data"]);
        expect(calls[0][2]).toEqual([3]);
    });
});
