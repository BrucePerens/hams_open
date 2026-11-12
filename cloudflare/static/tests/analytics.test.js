/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// The Cloudflare analytics dashboard component (CloudflareAnalyticsDashboard), mounted, under hoot's fake clock.
import { animationFrame } from "@odoo/hoot-dom";
import { advanceTime, afterEach, describe, expect, test } from "@odoo/hoot";
import { App } from "@odoo/owl";
import { getTemplate } from "@web/core/templates";
import { CloudflareAnalyticsDashboard } from "@cloudflare/components/analytics/analytics";

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

describe("cloudflare_analytics", () => {
    describe.current.tags("cloudflare_analytics");

    test("setup starts in the loading state and, once its half-second wait is over, shows the figures", async () => {
        // Tests [@ANCHOR: cloudflare:js_analytics_setup]
        const pending = mountPlain(CloudflareAnalyticsDashboard);
        await animationFrame();
        await advanceTime(500);
        const dashboard = await pending;
        expect(dashboard.state.loading).toBe(false);
        expect([dashboard.state.requests, dashboard.state.bandwidth, dashboard.state.threats]).toEqual([154200, "45.2 GB", 1205]);
        expect(document.body.textContent).toInclude("45.2 GB");
    });
});
