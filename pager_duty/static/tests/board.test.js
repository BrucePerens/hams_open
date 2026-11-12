/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// The NOC dashboard component (PagerBoard) against stand-ins for the orm and bus services, on a real mounted component.
import { afterEach, describe, expect, test } from "@odoo/hoot";
import { animationFrame } from "@odoo/hoot-dom";
import { App } from "@odoo/owl";
import { getTemplate } from "@web/core/templates";
import { patchWithCleanup } from "@web/../tests/web_test_helpers";
import { PagerBoard } from "@pager_duty/components/board/board";

const BOARD = {
    active: [{ id: 5, name: "Disk", source: "check", severity: "critical", status: "open" }],
    resolved: [{ id: 4, name: "Old", resolved_date: "x" }],
    on_duty: "K6BP",
    stats: { passing: 3, failing: 1, maintenance: 0, never_run: 2 },
};

/** Stand-ins for the two services; the orm records every call and answers get_board_data with BOARD. */
function stubServices() {
    const seen = { calls: [], channels: [], listeners: [] };
    seen.services = {
        orm: {
            call: async (model, method, args, kwargs) => {
                seen.calls.push([model, method, args, kwargs]);
                return method === "get_board_data" ? BOARD : true;
            },
        },
        bus_service: {
            addChannel: (name) => seen.channels.push(name),
            addEventListener: (name, handler) => seen.listeners.push([name, handler]),
        },
    };
    return seen;
}

/** A board object with the two services and state `setup()` leaves, without mounting it. */
function bareBoard(seen) {
    return Object.assign(Object.create(PagerBoard.prototype), {
        orm: { call: async (...args) => { seen.calls.push(args); return args[1] === "get_board_data" ? BOARD : true; } },
        state: { active_incidents: [], resolved_incidents: [], on_duty: "Loading...", stats: {}, transformStyle: "", isPaused: false },
    });
}

/** Mounts a component in a plain Owl app with only the given services: the full web client environment of mountWithCleanup starts services that call a server this suite lacks. */
async function mountPlain(Component, services = {}, props = {}) {
    const target = document.createElement("div");
    document.body.appendChild(target);
    const app = new App(Component, { env: { services }, props, getTemplate, translateFn: (text) => text });
    cleanups.push(() => { app.destroy(); target.remove(); });
    return app.mount(target);
}
const cleanups = [];
afterEach(() => { while (cleanups.length) { cleanups.pop()(); } });

describe("pager_duty_board", () => {
    describe.current.tags("pager_duty_board");

    test("setup subscribes to the pager_duty channel, loads the board and shows the incidents on screen", async () => {
        // Tests [@ANCHOR: pager_duty:js_board_setup]
        const seen = stubServices();
        const board = await mountPlain(PagerBoard, seen.services);
        await animationFrame();
        expect(seen.channels).toEqual(["pager_duty"]);
        expect(seen.listeners[0][0]).toBe("notification");
        expect(seen.calls[0].slice(0, 2)).toEqual(["pager.incident", "get_board_data"]);
        expect(board.state.on_duty).toBe("K6BP");
        expect(board.state.active_incidents.length).toBe(1);
        expect(document.body.textContent).toInclude("K6BP");
        expect(document.body.textContent).toInclude("Disk");
    });

    test("fetchData fills the state from the server's board data, passing the page's website id, and then shifts the display", async () => {
        // Tests [@ANCHOR: pager_duty:js_board_fetch_data]
        const seen = [];
        const board = bareBoard({ calls: seen });
        let shifted = 0;
        board.applyBurnInShift = () => { shifted += 1; };
        // The method reads document.location.search; compile it against a document whose address carries a website id.
        const fetchData = new Function("document", "URLSearchParams", `return (${"async function " + String(PagerBoard.prototype.fetchData).replace(/^async\s+/, "")});`)({ location: { search: "?website_id=7" } }, URLSearchParams);
        await fetchData.call(board);
        expect(seen[0]).toEqual(["pager.incident", "get_board_data", [], { context: { website_id: 7 } }]);
        await PagerBoard.prototype.fetchData.call(board);
        expect(seen[1][3]).toEqual({ context: {} });
        expect(board.state.stats).toEqual(BOARD.stats);
        expect(board.state.resolved_incidents).toEqual(BOARD.resolved);
        expect(shifted).toBe(2);
    });

    test("applyBurnInShift moves the display by up to 15 pixels each way over twenty seconds", () => {
        // Tests [@ANCHOR: pager_duty:js_board_apply_burn_in_shift]
        const board = bareBoard({ calls: [] });
        patchWithCleanup(Math, { random: () => 0 });
        board.applyBurnInShift();
        expect(board.state.transformStyle).toBe("transform: translate(-15px, -15px); transition: transform 20s linear;");
        patchWithCleanup(Math, { random: () => 0.9999 });
        board.applyBurnInShift();
        expect(board.state.transformStyle).toBe("transform: translate(14px, 14px); transition: transform 20s linear;");
    });

    test("acknowledge tells the server which incident and then reloads the board", async () => {
        // Tests [@ANCHOR: pager_duty:js_board_acknowledge]
        const seen = [];
        const board = bareBoard({ calls: seen });
        board.applyBurnInShift = () => {};
        await board.acknowledge(5);
        expect(seen.map((call) => call[1])).toEqual(["action_acknowledge", "get_board_data"]);
        expect(seen[0][2]).toEqual([[5]]);
    });

    test("togglePause flips between paused and live updates", () => {
        // Tests [@ANCHOR: pager_duty:js_board_toggle_pause]
        const board = bareBoard({ calls: [] });
        board.togglePause();
        expect(board.state.isPaused).toBe(true);
        board.togglePause();
        expect(board.state.isPaused).toBe(false);
    });

    test("onNotification reloads the board for an update_board message, ignores other messages and does nothing while paused", async () => {
        // Tests [@ANCHOR: pager_duty:js_board_on_notification]
        const board = bareBoard({ calls: [] });
        let fetches = 0;
        board.fetchData = async () => { fetches += 1; };
        board.onNotification({ detail: [{ type: "other" }] });
        expect(fetches).toBe(0);
        board.onNotification({ detail: [{ type: "update_board" }, { type: "update_board" }] });
        expect(fetches).toBe(2);
        board.state.isPaused = true;
        board.onNotification({ detail: [{ type: "update_board" }] });
        expect(fetches).toBe(2);
        board.state.isPaused = false;
        const logged = [];
        patchWithCleanup(console, { error: (...args) => logged.push(args) });
        board.fetchData = async () => { throw new Error("offline"); };
        board.onNotification({ detail: [{ type: "update_board" }] });
        await animationFrame();
        expect(logged.length).toBe(1);
    });
});
