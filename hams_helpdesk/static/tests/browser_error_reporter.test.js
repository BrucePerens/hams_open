/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { describe, expect, test } from "@odoo/hoot";
import {
    BrowserErrorReporter,
    ENDPOINT,
    MAX_FRAMES,
    MAX_REPORTS_PER_PAGE,
    agentFamily,
    buildReport,
    install,
    isIgnorable,
    normalizeRoute,
    pageEnv,
    reduceStack,
    releaseOf,
    sendReport,
    stripQuery,
    stripUrlQueries,
} from "@hams_helpdesk/js/browser_error_reporter";

const ORIGIN = "https://hams.example";
const BUNDLE = `${ORIGIN}/web/assets/1a2b3c4/web.assets_frontend.min.js`;
const ENV = {
    origin: ORIGIN,
    pathname: "/community/k6bp",
    userAgent: "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0",
    scriptUrls: [`${ORIGIN}/web/assets/1a2b3c4/web.assets_frontend.min.js`],
};

function errorWith(name, message, stack) {
    const error = new Error(message);
    error.name = name;
    error.stack = stack;
    return error;
}

const CHROME_STACK = [
    "TypeError: Cannot read properties of null (reading 'x')",
    `    at doThing (${BUNDLE}?secret=1:10:20)`,
    `    at ${ORIGIN}/web/assets/1a2b3c4/web.assets_frontend.min.js:30:40`,
    "    at <anonymous>",
].join("\n");
const FIREFOX_STACK = [`doThing@${BUNDLE}?x=1#frag:10:20`, `@${BUNDLE}:30:40`, "native@[native code]"].join("\n");

describe("hams_helpdesk browser error reporter", () => {
    describe.current.tags("hams_helpdesk");

    test("stripQuery drops the query and the fragment", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_strip_query]
        expect(stripQuery("https://a/b.js?x=1#y")).toBe("https://a/b.js");
        expect(stripQuery("/a/b")).toBe("/a/b");
        expect(stripQuery(null)).toBe("");
    });

    test("reduceStack reads Chrome and Firefox frames without query strings", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_reduce_stack]
        expect(reduceStack(CHROME_STACK)).toEqual([`${BUNDLE}:10:20`, `${BUNDLE}:30:40`]);
        expect(reduceStack(FIREFOX_STACK)).toEqual([`${BUNDLE}:10:20`, `${BUNDLE}:30:40`]);
        const long = Array.from({ length: 20 }, (_, i) => `    at f${i} (${BUNDLE}:${i + 1}:1)`).join("\n");
        expect(reduceStack(long).length).toBe(MAX_FRAMES);
        expect(reduceStack("")).toEqual([]);
        for (const frame of reduceStack(CHROME_STACK)) {
            expect(frame.includes("?")).toBe(false);
            expect(frame.includes("secret")).toBe(false);
        }
    });

    test("normalizeRoute replaces variable segments and keeps no query", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_normalize_route]
        expect(normalizeRoute("/community/k6bp")).toBe("/community/<x>");
        expect(normalizeRoute("/event/page/2?search=a@b.com")).toBe("/event/page/<x>");
        expect(normalizeRoute("/my/tickets")).toBe("/my/tickets");
        expect(normalizeRoute("/a/" + "z".repeat(30))).toBe("/a/<x>");
        expect(normalizeRoute("")).toBe("/");
    });

    test("agentFamily gives only a family from the fixed list", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_agent_family]
        expect(agentFamily(ENV.userAgent)).toBe("Firefox");
        expect(agentFamily("Mozilla/5.0 Chrome/130.0 Safari/537.36")).toBe("Chrome");
        expect(agentFamily("Mozilla/5.0 Chrome/130.0 Safari/537.36 Edg/130.0")).toBe("Edge");
        expect(agentFamily("Mozilla/5.0 Version/17.0 Safari/605.1.15")).toBe("Safari");
        expect(agentFamily("curl/8.0")).toBe("Other");
    });

    test("releaseOf reads the bundle from the failing frame, else from the page's scripts", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_release_of]
        expect(releaseOf(`${BUNDLE}:1:2`, [])).toBe("web.assets_frontend@1a2b3c4");
        expect(releaseOf(`${ORIGIN}/foo/static/a.js:1:2`, [`${ORIGIN}/web/assets/9f8e7d6/web.assets_backend.min.js?x`])).toBe(
            "web.assets_backend@9f8e7d6"
        );
        expect(releaseOf(`${ORIGIN}/foo/static/a.js:1:2`, [])).toBe(null);
    });

    test("isIgnorable skips RPC errors, dropped connections and ResizeObserver noise", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_is_ignorable]
        expect(isIgnorable("RPC_ERROR", "x")).toBe(true);
        expect(isIgnorable("TypeError", "Failed to fetch")).toBe(true);
        expect(isIgnorable("Error", "ResizeObserver loop completed with undelivered notifications.")).toBe(true);
        expect(isIgnorable("Error", "x", { exceptionName: "odoo.exceptions.UserError" })).toBe(true);
        expect(isIgnorable("TypeError", "Cannot read properties of null")).toBe(false);
    });

    test("stripUrlQueries cuts the query out of a URL quoted in a message", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_strip_url_queries]
        expect(stripUrlQueries("Failed https://a.b/c?token=zzz&x=1 now")).toBe("Failed https://a.b/c now");
        expect(stripUrlQueries("no url")).toBe("no url");
    });

    test("buildReport holds only the allowed fields and drops queries and foreign scripts", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_build_report]
        const error = errorWith("TypeError", "bad https://x.y/z?email=a@b.com " + "m".repeat(400), CHROME_STACK);
        const report = buildReport(error, null, ENV);
        expect(Object.keys(report).sort()).toEqual(["agent", "frames", "message", "release", "route", "type", "v"]);
        expect(report.type).toBe("TypeError");
        expect(report.message.length <= 300).toBe(true);
        expect(report.message.includes("email=")).toBe(false);
        expect(report.frames).toEqual([`${BUNDLE}:10:20`, `${BUNDLE}:30:40`]);
        expect(report.route).toBe("/community/<x>");
        expect(report.agent).toBe("Firefox");
        expect(report.release).toBe("web.assets_frontend@1a2b3c4");
        expect(JSON.stringify(report).includes("secret")).toBe(false);
        // A browser extension's error is not ours.
        const foreign = errorWith("Error", "x", "Error: x\n    at f (chrome-extension://abc/content.js:1:2)");
        expect(buildReport(foreign, null, ENV)).toBe(null);
        // "Script error." with no stack and no file name says nothing.
        expect(buildReport(null, { filename: "", lineno: 0, colno: 0 }, ENV)).toBe(null);
        // No stack: the event's own file name is used.
        const noStack = errorWith("TypeError", "x", "");
        expect(buildReport(noStack, { filename: `${BUNDLE}?v=1`, lineno: 5, colno: 6 }, ENV).frames).toEqual([
            `${BUNDLE}:5:6`,
        ]);
    });

    test("sendReport posts JSON without cookies and swallows a failure", async () => {
        // Tests [@ANCHOR: hams_helpdesk:js_send_report]
        const calls = [];
        const ok = (url, options) => {
            calls.push([url, options]);
            return Promise.resolve({ ok: true });
        };
        await sendReport({ v: 1 }, ok);
        expect(calls[0][0]).toBe(ENDPOINT);
        expect(calls[0][1].method).toBe("POST");
        expect(calls[0][1].credentials).toBe("same-origin");
        expect(calls[0][1].headers["Content-Type"]).toBe("application/json");
        expect(calls[0][1].body).toBe('{"v":1}');
        const failing = () => Promise.reject(new Error("offline"));
        expect(await sendReport({ v: 1 }, failing)).toBe(null);
    });

    test("the reporter sends once per error, at most five per page, and spaces them out", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_reporter_construct]
        // Tests [@ANCHOR: hams_helpdesk:js_reporter_report]
        // Tests [@ANCHOR: hams_helpdesk:js_reporter_env]
        const sent = [];
        let clock = 0;
        const reporter = new BrowserErrorReporter({ env: ENV, send: (r) => sent.push(r), now: () => clock });
        const make = (line) => errorWith("TypeError", "x", `TypeError: x\n    at f (${BUNDLE}:${line}:1)`);
        expect(reporter.report(make(1))).toBe(true);
        // The same error again: de-duplicated.
        clock += 5000;
        expect(reporter.report(make(1))).toBe(false);
        // A different error inside the minimum interval: dropped.
        expect(reporter.report(make(2), null)).toBe(true);
        clock += 10;
        expect(reporter.report(make(3))).toBe(false);
        for (let line = 4; line < 20; line++) {
            clock += 5000;
            reporter.report(make(line));
        }
        expect(sent.length).toBe(MAX_REPORTS_PER_PAGE);
        // Never throws, even when the sender does.
        const broken = new BrowserErrorReporter({
            env: ENV,
            send: () => {
                throw new Error("boom");
            },
        });
        expect(broken.report(make(1))).toBe(false);
        expect(broken.busy).toBe(false);
    });

    test("install listens for error and unhandledrejection and reports through the reporter", () => {
        // Tests [@ANCHOR: hams_helpdesk:js_install]
        // Tests [@ANCHOR: hams_helpdesk:js_reporter_on_error]
        // Tests [@ANCHOR: hams_helpdesk:js_reporter_on_rejection]
        // Tests [@ANCHOR: hams_helpdesk:js_page_env]
        const listeners = {};
        const win = {
            addEventListener: (name, handler) => (listeners[name] = handler),
            location: { origin: ORIGIN, pathname: "/my/tickets" },
            navigator: { userAgent: ENV.userAgent },
            document: { querySelectorAll: () => [{ src: `${BUNDLE}` }] },
        };
        expect(pageEnv(win)).toEqual({
            origin: ORIGIN,
            pathname: "/my/tickets",
            userAgent: ENV.userAgent,
            scriptUrls: [BUNDLE],
        });
        const sent = [];
        install(win, { send: (r) => sent.push(r) });
        expect(Object.keys(listeners).sort()).toEqual(["error", "unhandledrejection"]);
        listeners.error({ error: errorWith("TypeError", "a", `TypeError: a\n    at f (${BUNDLE}:7:8)`) });
        expect(sent.length).toBe(1);
        expect(sent[0].frames[0]).toBe(`${BUNDLE}:7:8`);
        // A rejection with a string reason has no stack: nothing to report.
        listeners.unhandledrejection({ reason: "nope" });
        expect(sent.length).toBe(1);
        listeners.unhandledrejection({
            reason: errorWith("RangeError", "b", `RangeError: b\n    at g (${BUNDLE}:9:9)`),
        });
        expect(sent.length).toBe(1); // inside the one-second spacing
    });
});
