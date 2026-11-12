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
// Python tour registered under the test anchor test_tour_toast_notifications
// (user_websites/tests/test_ui_tours.py) -- matching adif_uploader.test.js's
// own precedent of skipping branches a hoot unit test can't safely reach
// and relying on the tour instead.
import { describe, expect, mockFetch, test } from "@odoo/hoot";
import { AdminViolationToast, MODERATION_TOAST_KEY, MODERATION_TOAST_TTL_MS, REPORT_ERRORS, UrlToastNotification, readSeen, shouldShowToast, writeSeen } from "@user_websites/js/toast_notifications";

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
        // Tests [@ANCHOR: user_websites:js_toast_notifications_check_pending_reports]
        // Tests [@ANCHOR: user_websites:js_toast_notifications_write_seen]
        // Tests [@ANCHOR: user_websites:js_toast_notifications_read_seen]
        localStorage.removeItem(MODERATION_TOAST_KEY);
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        mockFetch(() => ({ count: 3, latest_id: 7, url: "/odoo/action-x" }));
        instance._checkPendingReports();
        await flushMicrotasks();

        expect(calls.length).toBe(1);
        expect(calls[0].message).toBe("There are 3 pending violation reports requiring review.");
        expect(calls[0].opts.type).toBe("warning");
        expect(calls[0].opts.sticky).toBe(true);
        expect(calls[0].opts.buttons.map((b) => b.name)).toEqual(["Review reports", "Dismiss"]);
        expect(JSON.parse(localStorage.getItem(MODERATION_TOAST_KEY)).count).toBe(3);
        localStorage.removeItem(MODERATION_TOAST_KEY);
    });

    test("AdminViolationToast stays silent when the count is zero", async () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_check_pending_reports]
        localStorage.removeItem(MODERATION_TOAST_KEY);
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        mockFetch(() => ({ count: 0 }));
        instance._checkPendingReports();
        await flushMicrotasks();

        expect(calls.length).toBe(0);
        expect(localStorage.getItem(MODERATION_TOAST_KEY)).toBe(null);
    });

    test("AdminViolationToast swallows a network failure instead of throwing", async () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_check_pending_reports]
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        mockFetch(() => { throw new Error("network down"); });
        // Must not throw/reject, and must not surface a toast either.
        instance._checkPendingReports();
        await flushMicrotasks();

        expect(calls.length).toBe(0);
    });

    test("AdminViolationToast is not shown twice for the same queue, but is when it grows", async () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_check_pending_reports]
        // Tests [@ANCHOR: user_websites:js_toast_notifications_write_seen]
        // Tests [@ANCHOR: user_websites:js_toast_notifications_read_seen]
        localStorage.removeItem(MODERATION_TOAST_KEY);
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        mockFetch(() => ({ count: 3, latest_id: 7, url: "/odoo/action-x" }));
        await instance._checkPendingReports();
        await instance._checkPendingReports();
        expect(calls.length).toBe(1);

        mockFetch(() => ({ count: 4, latest_id: 8, url: "/odoo/action-x" }));
        await instance._checkPendingReports();
        expect(calls.length).toBe(2);
        localStorage.removeItem(MODERATION_TOAST_KEY);
    });

    test("shouldShowToast: skip, increase and 12h expiry", () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_should_show_toast]
        const seen = { count: 3, latest_id: 7, ts: 1000 };
        const same = { count: 3, latest_id: 7 };
        expect(shouldShowToast(null, same, 2000)).toBe(true);
        expect(shouldShowToast(seen, same, 2000)).toBe(false);
        expect(shouldShowToast(seen, { count: 2, latest_id: 7 }, 2000)).toBe(false);
        expect(shouldShowToast(seen, { count: 4, latest_id: 7 }, 2000)).toBe(true);
        expect(shouldShowToast(seen, { count: 3, latest_id: 9 }, 2000)).toBe(true);
        expect(shouldShowToast(seen, same, 1000 + MODERATION_TOAST_TTL_MS)).toBe(true);
        expect(shouldShowToast(seen, { count: 0 }, 2000)).toBe(false);
    });

    test("both toasts check on start", () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_url_start]
        // Tests [@ANCHOR: user_websites:js_toast_notifications_admin_start]
        const checked = [];
        UrlToastNotification.prototype.start.call({ _checkUrlForNotifications: () => checked.push("url") });
        AdminViolationToast.prototype.start.call({ _checkPendingReports: () => { checked.push("admin"); return Promise.resolve(); } });
        expect(checked).toEqual(["url", "admin"]);
    });

    /** Runs _checkUrlForNotifications() on a page whose address is `search`; the method reads document.location, which a test
     * cannot set, so it is compiled with a stand-in document and window (the same device hams_com's event tests use). */
    const checkUrl = (search) => {
        const seen = { notices: [], replaced: [] };
        const scope = {
            document: { location: { search, protocol: "https:", host: "example.org", pathname: "/site/page" } },
            window: { history: { replaceState: (...args) => seen.replaced.push(args) } },
            REPORT_ERRORS,
        };
        const method = new Function(...Object.keys(scope), `return (${"function " + String(UrlToastNotification.prototype._checkUrlForNotifications)});`)(...Object.values(scope));
        method.call({ env: { services: { notification: { add: (message, options) => seen.notices.push([message, options]) } } } });
        return seen;
    };

    test("_checkUrlForNotifications shows the notification for each success parameter and cleans the address", () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_check_url]
        const cases = {
            report_submitted: ["Success", "We received your report and will review it."],
            appeal_submitted: ["Submitted", "We received your appeal and are reviewing it."],
            subscribed: ["Subscribed", "You will now receive weekly email digests for this site."],
            erased: ["Content Deleted", "We permanently erased your personal pages and blog posts from our servers."],
        };
        for (const [parameter, [title, message]] of Object.entries(cases)) {
            const seen = checkUrl(`?${parameter}=1`);
            expect(seen.notices).toEqual([[message, { title, type: "success", sticky: false }]]);
            expect(seen.replaced).toEqual([[{ path: "https://example.org/site/page" }, "", "https://example.org/site/page"]]);
        }
    });

    test("_checkUrlForNotifications explains a refused report in a danger notification, with the default words for an unknown reason", () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_check_url]
        const known = checkUrl("?report_error=too_fast");
        expect(known.notices).toEqual([[REPORT_ERRORS.too_fast, { title: "Report not sent", type: "danger", sticky: false }]]);
        expect(checkUrl("?report_error=made_up").notices[0][0]).toBe(REPORT_ERRORS.default);
    });

    test("_checkUrlForNotifications does nothing on a page without those parameters", () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_check_url]
        const seen = checkUrl("?page=2");
        expect(seen.notices.length).toBe(0);
        expect(seen.replaced.length).toBe(0);
    });

    test("readSeen returns what was stored, null when nothing is stored, and null when the storage refuses", () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_read_seen]
        const stored = { count: 2, latest_id: 5, ts: 10 };
        expect(readSeen({ getItem: (key) => (key === MODERATION_TOAST_KEY ? JSON.stringify(stored) : null) })).toEqual(stored);
        expect(readSeen({ getItem: () => null })).toBe(null);
        expect(readSeen({ getItem: () => "{not json" })).toBe(null);
        expect(readSeen({ getItem: () => { throw new Error("blocked"); } })).toBe(null);
    });

    test("writeSeen stores the count, newest id and time, and ignores blocked storage", () => {
        // Tests [@ANCHOR: user_websites:js_toast_notifications_write_seen]
        const written = {};
        writeSeen({ count: 3, latest_id: 7 }, 1234, { setItem: (key, value) => { written[key] = JSON.parse(value); } });
        expect(written[MODERATION_TOAST_KEY]).toEqual({ count: 3, latest_id: 7, ts: 1234 });
        writeSeen({ count: 1 }, 5, { setItem: (key, value) => { written[key] = JSON.parse(value); } });
        expect(written[MODERATION_TOAST_KEY].latest_id).toBe(0);
        expect(() => writeSeen({ count: 1 }, 5, { setItem: () => { throw new Error("blocked"); } })).not.toThrow();
    });
});
