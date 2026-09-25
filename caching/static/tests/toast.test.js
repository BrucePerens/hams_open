/** @odoo-module **/
/** Copyright © HAMS project. AGPL-3.0-or-later. **/

// SWToast is almost entirely template/service-worker-event wiring; close()
// and _showOnceModalClear() are its independently-testable state logic.
// reload() is not tested -- it only calls document.location.reload(), a
// real browser navigation with no branching logic of its own to verify.
import { afterEach, describe, expect, test } from "@odoo/hoot";
import { SWToast } from "@caching/js/toast";

describe("toast", () => {
    describe.current.tags("caching_toast");

    test("close() hides the toast by setting state.show to false", () => {
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
});
