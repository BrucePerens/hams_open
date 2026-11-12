/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// The security dashboard component (SecurityDashboard) against stand-ins for the orm and action services, on a real mounted
// component; the display shift runs under hoot's fake clock.
import { advanceTime, afterEach, describe, expect, test } from "@odoo/hoot";
import { animationFrame } from "@odoo/hoot-dom";
import { App } from "@odoo/owl";
import { getTemplate } from "@web/core/templates";
import { patchWithCleanup } from "@web/../tests/web_test_helpers";
import { SecurityDashboard } from "@zero_sudo/components/security_dashboard/security_dashboard";

const LOGS = [{ id: 1, create_date: "2026-11-11 10:00:00", user_id: false, login: "mallory", ip_address: "203.0.113.9", reason: "bad password" }];

function stubServices() {
    const calls = [];
    const services = {
        orm: {
            searchRead: async (...args) => {
                calls.push(args);
                return LOGS;
            },
        },
        action: {},
    };
    return { calls, services };
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

describe("zero_sudo_security_dashboard", () => {
    describe.current.tags("zero_sudo_security_dashboard");

    test("setup loads the logs before showing, and shifts the display a few pixels every twenty seconds unless paused", async () => {
        // Tests [@ANCHOR: zero_sudo:js_security_dashboard_setup]
        const { services } = stubServices();
        patchWithCleanup(Math, { random: () => 0 });
        const dashboard = await mountPlain(SecurityDashboard, services);
        expect(dashboard.state.logs).toEqual(LOGS);
        expect(document.body.textContent).toInclude("mallory");
        const root = dashboard.dashboardRoot.el;
        await advanceTime(20000);
        expect(root.style.transform).toBe("translate(-3px, -3px)");
        dashboard.state.isPaused = true;
        root.style.transform = "";
        await advanceTime(20000);
        expect(root.style.transform).toBe("");
    });

    test("loadLogs reads the newest hundred security log entries", async () => {
        // Tests [@ANCHOR: zero_sudo:js_security_dashboard_load_logs]
        const { calls, services } = stubServices();
        const dashboard = await mountPlain(SecurityDashboard, services);
        await animationFrame();
        calls.length = 0;
        dashboard.state.logs = [];
        await dashboard.loadLogs();
        expect(calls[0].slice(0, 2)).toEqual(["zero_sudo.security.log", []]);
        expect(calls[0][2]).toEqual(["id", "create_date", "user_id", "login", "ip_address", "reason"]);
        expect(calls[0][3]).toEqual({ limit: 100, order: "create_date desc" });
        expect(dashboard.state.logs).toEqual(LOGS);
    });

    test("togglePause flips between paused and live", () => {
        // Tests [@ANCHOR: zero_sudo:js_security_dashboard_toggle_pause]
        const fake = Object.assign(Object.create(SecurityDashboard.prototype), { state: { isPaused: false } });
        fake.togglePause();
        expect(fake.state.isPaused).toBe(true);
        fake.togglePause();
        expect(fake.state.isPaused).toBe(false);
    });
});
