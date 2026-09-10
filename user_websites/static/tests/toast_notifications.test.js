/** @odoo-module **/

// AdminViolationToast reads global browser state directly (sessionStorage,
// fetch), so these tests drive that real global state (a real
// sessionStorage key, a mocked fetch) rather than faking the class's own
// internals -- its real logic is exactly "what do we do with this real
// browser state", so faking the state out from under it would test less,
// not more. fetch is mocked through hoot's sanctioned mockFetch() hook
// rather than reassigning window.fetch directly -- hoot's mocked window
// exposes `fetch` as a read-only property (odoo/addons/web/static/lib/
// hoot/mock/network.js), so a direct assignment throws before any
// assertion runs. No manual restore needed -- hoot resets mocks between
// tests automatically.
//
// UrlToastNotification._checkUrlForNotifications() is NOT covered here --
// it reads document.location.search directly, which cannot be faked in
// this hoot unit test bundle. Two things were verified while wiring this
// suite up, together making it genuinely untestable here (not just
// awkward):
//   1. hoot's mocked window only redirects `window.history` to a
//      MockHistory/MockLocation pair (odoo/addons/web/static/lib/hoot/
//      mock/window.js's WINDOW_MOCK_DESCRIPTORS -- `history: { value:
//      mockHistory }`); `window.location`/`document.location` are never
//      patched (no `location` entry there at all), so
//      `window.history.pushState(...)` -- and _checkUrlForNotifications's
//      own `window.history.replaceState(...)` cleanup call -- write to
//      the mock, while `document.location.search` reads the real,
//      unrelated live location of the hoot runner's own page.
//   2. There's no way around that split by patching document.location
//      directly either: `Object.defineProperty(location, "search", ...)`
//      throws "Cannot redefine property: search" in this browser --
//      confirmed live, `Object.getOwnPropertyDescriptor(location,
//      "search")` reports `configurable: false` (true of every Location
//      accessor property tested: href, pathname, origin, reload). Odoo's
//      own core test suite papers over exactly this gap with
//      `patchWithCleanup(browser.location, {...})` (e.g. web/static/
//      tests/webclient/actions/push_state.test.js), but that helper --
//      and whatever upstream fixture makes `browser.location` patchable
//      for it -- lives under web/static/tests/_framework, which isn't
//      part of the web.assets_unit_tests_setup bundle this project's hoot
//      suites run in.
// This behavior is instead covered end to end, in a real browser, by the
// Python tour at [@ANCHOR: test_tour_toast_notifications]
// (user_websites/tests/test_ui_tours.py) -- matching adif_uploader.test.js's
// own precedent of skipping branches a hoot unit test can't safely reach
// and relying on the tour instead.
import { describe, expect, mockFetch, test } from "@odoo/hoot";
import { AdminViolationToast } from "@user_websites/js/toast_notifications";

// _checkPendingReports() never returns its own fetch(...).then().then()
// chain (no `return` in the source), so awaiting its call directly
// resolves before that chain has actually run. Verified with a standalone
// Node reproduction of the exact same shape (a fetch-returning function,
// two chained .then()s, a .catch()): the chain's own callback genuinely
// hasn't fired yet immediately after the call returns, but has after a
// setTimeout(0) macrotask-boundary flush -- not assumed from how
// microtasks are generally supposed to work.
function flushMicrotasks() {
    return new Promise((resolve) => setTimeout(resolve, 0));
}

describe("toast_notifications", () => {
    describe.current.tags("user_websites_toast_notifications");

    test("AdminViolationToast surfaces a warning toast when the API reports pending reports", async () => {
        sessionStorage.removeItem("admin_violation_toast_shown");
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        mockFetch(() => ({ count: 3 }));
        instance._checkPendingReports();
        await flushMicrotasks();

        expect(calls.length).toBe(1);
        expect(calls[0].message).toBe("There are 3 pending violation reports requiring review.");
        expect(calls[0].opts.type).toBe("warning");
        expect(sessionStorage.getItem("admin_violation_toast_shown")).toBe("true");
        sessionStorage.removeItem("admin_violation_toast_shown");
    });

    test("AdminViolationToast stays silent when the count is zero", async () => {
        sessionStorage.removeItem("admin_violation_toast_shown");
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        mockFetch(() => ({ count: 0 }));
        instance._checkPendingReports();
        await flushMicrotasks();

        expect(calls.length).toBe(0);
        expect(sessionStorage.getItem("admin_violation_toast_shown")).toBe(null);
    });

    test("AdminViolationToast swallows a network failure instead of throwing", async () => {
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        mockFetch(() => { throw new Error("network down"); });
        // Must not throw/reject, and must not surface a toast either.
        instance._checkPendingReports();
        await flushMicrotasks();

        expect(calls.length).toBe(0);
    });
});
