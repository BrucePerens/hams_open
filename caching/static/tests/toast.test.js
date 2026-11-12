/** @odoo-module **/
/** Copyright © HAMS project. AGPL-3.0-or-later. **/

// SWToast is almost entirely template/service-worker-event wiring; close()
// and _showOnceModalClear() are its independently-testable state logic.
// reload() is not tested -- it only calls document.location.reload(), a
// real browser navigation with no branching logic of its own to verify.
import { afterEach, describe, expect, test } from "@odoo/hoot";
import { animationFrame } from "@odoo/hoot-dom";
import { App } from "@odoo/owl";
import { getTemplate } from "@web/core/templates";
import { SWToast } from "@caching/js/toast";

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

describe("toast", () => {
    describe.current.tags("caching_toast");

    test("close() hides the toast by setting state.show to false", () => {
        // Tests [@ANCHOR: caching:js_toast_close]
        const instance = Object.create(SWToast.prototype);
        instance.state = { show: true };
        instance.close();
        expect(instance.state.show).toBe(false);
    });

    // [@ANCHOR: test_toast_deferred_while_modal_open]
    // Tests [@ANCHOR: caching:toast_deferred_while_modal_open]
    let modalEl;
    afterEach(() => {
        if (modalEl) {
            modalEl.remove();
            modalEl = null;
        }
    });

    test("_showOnceModalClear() shows immediately when no modal is open", () => {
        const instance = Object.create(SWToast.prototype);
        instance.state = { show: false };
        instance._showOnceModalClear();
        expect(instance.state.show).toBe(true);
    });

    test("_showOnceModalClear() waits for an open modal to close first", () => {
        modalEl = document.createElement("div");
        modalEl.className = "modal show";
        document.body.appendChild(modalEl);

        const instance = Object.create(SWToast.prototype);
        instance.state = { show: false };
        instance._showOnceModalClear();
        // Real bug this guards against: the toast used to appear immediately,
        // covering the still-open modal's own buttons -- it must not show
        // while `.modal.show` is still in the DOM.
        expect(instance.state.show).toBe(false);

        // Real Bootstrap removes the "show" class before it fires
        // hidden.bs.modal, not after -- mirror that ordering here.
        modalEl.classList.remove("show");
        document.dispatchEvent(new Event("hidden.bs.modal"));
        expect(instance.state.show).toBe(true);
    });

    test("setup() shows the toast when the service worker says a new version is installed, and ignores other messages", async () => {
        // Tests [@ANCHOR: caching:js_toast_setup]
        const toast = await mountPlain(SWToast);
        const send = (data) => navigator.serviceWorker.dispatchEvent(new MessageEvent("message", { data }));
        expect(toast.state.show).toBe(false);
        send({ type: "SOMETHING_ELSE" });
        send(null);
        await animationFrame();
        expect(toast.state.show).toBe(false);
        send({ type: "NEW_VERSION_INSTALLED" });
        await animationFrame();
        expect(toast.state.show).toBe(true);
        expect(document.querySelector(".toast-body").textContent).toInclude("A new version of this app is available.");
        document.querySelector(".btn-close").click();
        await animationFrame();
        expect(toast.state.show).toBe(false);
    });

    test("reload() reloads the page", () => {
        // Tests [@ANCHOR: caching:js_toast_reload]
        let reloads = 0;
        const reload = new Function("document", `return (${String(SWToast.prototype.reload).replace(/^/, "function ")});`)({ location: { reload: () => { reloads += 1; } } });
        reload.call(null);
        expect(reloads).toBe(1);
    });
});
