/** @odoo-module **/

// Both classes read global browser state directly (document.location,
// sessionStorage, fetch) rather than taking it as a parameter, so these
// tests drive that real global state (a real history.pushState to set the
// URL query string, a real sessionStorage key, a temporarily-stubbed
// window.fetch) rather than faking the classes' own internals -- the
// classes' real logic is exactly "what do we do with this real browser
// state", so faking the state out from under it would test less, not more.
import { afterEach, beforeEach, describe, expect, test } from "@odoo/hoot";
import { UrlToastNotification, AdminViolationToast } from "@user_websites/js/toast_notifications";

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

function makeInstanceWithNotificationSpy() {
    const calls = [];
    const instance = Object.create(UrlToastNotification.prototype);
    instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };
    return { instance, calls };
}

describe("toast_notifications", () => {
    describe.current.tags("user_websites_toast_notifications");

    let originalUrl;
    beforeEach(() => {
        originalUrl = document.location.pathname + document.location.search;
    });
    afterEach(() => {
        window.history.replaceState({}, "", originalUrl);
    });

    test("report_submitted fires a success toast and cleans the URL", () => {
        window.history.pushState({}, "", "?report_submitted=1");
        const { instance, calls } = makeInstanceWithNotificationSpy();

        instance._checkUrlForNotifications();

        expect(calls.length).toBe(1);
        expect(calls[0].opts.title).toBe("Success");
        expect(calls[0].message).toBe("We received your report and will review it.");
        expect(document.location.search).toBe("");
    });

    test("appeal_submitted, subscribed, and erased each map to their own distinct message", () => {
        const cases = [
            ["appeal_submitted", "Submitted"],
            ["subscribed", "Subscribed"],
            ["erased", "Content Deleted"],
        ];
        for (const [param, expectedTitle] of cases) {
            window.history.pushState({}, "", `?${param}=1`);
            const { instance, calls } = makeInstanceWithNotificationSpy();
            instance._checkUrlForNotifications();
            expect(calls.length).toBe(1);
            expect(calls[0].opts.title).toBe(expectedTitle);
        }
    });

    test("no recognized query param means no notification and the URL is left alone", () => {
        window.history.pushState({}, "", "?utm_source=newsletter");
        const { instance, calls } = makeInstanceWithNotificationSpy();

        instance._checkUrlForNotifications();

        expect(calls.length).toBe(0);
        expect(document.location.search).toBe("?utm_source=newsletter");
    });

    test("AdminViolationToast surfaces a warning toast when the API reports pending reports", async () => {
        sessionStorage.removeItem("admin_violation_toast_shown");
        const calls = [];
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: (message, opts) => calls.push({ message, opts }) } } };

        const originalFetch = window.fetch;
        window.fetch = async () => ({ ok: true, json: async () => ({ count: 3 }) });
        try {
            instance._checkPendingReports();
            await flushMicrotasks();
        } finally {
            window.fetch = originalFetch;
        }

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

        const originalFetch = window.fetch;
        window.fetch = async () => ({ ok: true, json: async () => ({ count: 0 }) });
        try {
            instance._checkPendingReports();
            await flushMicrotasks();
        } finally {
            window.fetch = originalFetch;
        }

        expect(calls.length).toBe(0);
        expect(sessionStorage.getItem("admin_violation_toast_shown")).toBe(null);
    });

    test("AdminViolationToast swallows a network failure instead of throwing", async () => {
        const instance = Object.create(AdminViolationToast.prototype);
        instance.env = { services: { notification: { add: () => {} } } };

        const originalFetch = window.fetch;
        window.fetch = async () => { throw new Error("network down"); };
        try {
            // Must not throw/reject.
            instance._checkPendingReports();
            await flushMicrotasks();
        } finally {
            window.fetch = originalFetch;
        }
    });
});
