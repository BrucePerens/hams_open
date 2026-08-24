/** @odoo-module **/
/** Copyright © HAMS project. AGPL-3.0. **/

// SWToast is almost entirely template/service-worker-event wiring; close()
// is its one piece of independently-testable state logic. reload() is not
// tested -- it only calls document.location.reload(), a real browser
// navigation with no branching logic of its own to verify.
import { describe, expect, test } from "@odoo/hoot";
import { SWToast } from "@caching/js/toast";

describe("toast", () => {
    describe.current.tags("caching_toast");

    test("close() hides the toast by setting state.show to false", () => {
        const instance = Object.create(SWToast.prototype);
        instance.state = { show: true };
        instance.close();
        expect(instance.state.show).toBe(false);
    });
});
