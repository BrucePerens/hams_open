/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// The log search page (LogViewer) with a stubbed search API. The search waits a second between polls, so those tests run
// under hoot's fake clock and advance it.
import { advanceTime, afterEach, describe, expect, mockFetch, test } from "@odoo/hoot";
import { animationFrame } from "@odoo/hoot-dom";
import { App } from "@odoo/owl";
import { getTemplate } from "@web/core/templates";
import { LogViewer } from "@pager_duty/components/log_viewer/log_viewer";

/** A viewer object with the state `setup()` leaves, without mounting it. */
function bareViewer(overrides = {}) {
    return Object.assign(Object.create(LogViewer.prototype), {
        state: { files: [], selectedFile: "/var/log/a.log", regexQuery: "ERROR", results: [], loading: false, error: null },
    }, overrides);
}

/** Answers each API path in turn from `replies` (a map from URL to a reply, or to a list of replies used in order). */
function stubApi(replies) {
    const requests = [];
    mockFetch((url, init) => {
        requests.push([url, JSON.parse(init.body).params]);
        const reply = replies[url];
        return Array.isArray(reply) ? reply.shift() : reply;
    });
    return requests;
}

/** Lets the search's one-second waits pass on the fake clock until it has finished. */
async function finish(promise) {
    let done = false;
    promise.then(() => { done = true; });
    for (let second = 0; second < 40 && !done; second++) {
        await animationFrame();
        await advanceTime(1000);
    }
    await promise;
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

describe("pager_duty_log_viewer", () => {
    describe.current.tags("pager_duty_log_viewer");

    test("setup loads the list of log files when mounted", async () => {
        // Tests [@ANCHOR: pager_duty_js_log_viewer_log_viewer]
        // Tests [@ANCHOR: pager_duty:js_log_viewer_setup]
        const requests = stubApi({ "/api/v1/pager/logs/files": { result: { files: ["/var/log/a.log", "/var/log/b.log"] } } });
        const viewer = await mountPlain(LogViewer, { orm: {} });
        await animationFrame();
        expect(requests.length).toBe(1);
        expect(viewer.state.files).toEqual(["/var/log/a.log", "/var/log/b.log"]);
    });

    test("fetchFiles lists the files and picks the first, and says so when the list cannot be loaded", async () => {
        // Tests [@ANCHOR: pager_duty:js_log_viewer_fetch_files]
        stubApi({ "/api/v1/pager/logs/files": [{ result: { files: ["x.log", "y.log"] } }, { result: {} }, new Response("not json")] });
        const viewer = bareViewer();
        await viewer.fetchFiles();
        expect([viewer.state.files, viewer.state.selectedFile]).toEqual([["x.log", "y.log"], "x.log"]);
        const empty = bareViewer({ state: { files: [], selectedFile: "" } });
        await empty.fetchFiles();
        expect([empty.state.files, empty.state.selectedFile]).toEqual([[], ""]);
        const broken = bareViewer({ state: { files: [], selectedFile: "", error: null } });
        await broken.fetchFiles();
        expect(broken.state.error).toBe("Failed to load configuration.");
    });

    test("onKeyup starts the search on Enter only", () => {
        // Tests [@ANCHOR: pager_duty:js_log_viewer_on_keyup]
        let searches = 0;
        const viewer = bareViewer({ executeSearch: async () => { searches += 1; } });
        viewer.onKeyup({ key: "a" });
        viewer.onKeyup({ key: "Enter" });
        expect(searches).toBe(1);
    });

    test("executeSearch asks for a file and a pattern first", async () => {
        // Tests [@ANCHOR: pager_duty:js_log_viewer_execute_search]
        const requests = stubApi({});
        const viewer = bareViewer();
        viewer.state.regexQuery = "";
        await viewer.executeSearch();
        expect(viewer.state.error).toBe("Please select a file and enter a regex pattern.");
        expect(requests.length).toBe(0);
    });

    test("executeSearch starts a job, polls until it is done, and shows the matches", async () => {
        // Tests [@ANCHOR: pager_duty:js_log_viewer_execute_search]
        const requests = stubApi({
            "/api/v1/pager/logs/search": { result: { job_id: "j1" } },
            "/api/v1/pager/logs/search_poll": [{ result: { status: "running" } }, { result: { status: "done", matches: ["line 1", "line 2"] } }],
        });
        const viewer = bareViewer();
        const search = viewer.executeSearch();
        await animationFrame();
        expect(viewer.state.loading).toBe(true);
        await finish(search);
        expect(requests[0]).toEqual(["/api/v1/pager/logs/search", { file_path: "/var/log/a.log", regex_query: "ERROR" }]);
        expect(requests.slice(1).map((request) => request[1])).toEqual([{ job_id: "j1" }, { job_id: "j1" }]);
        expect(viewer.state.results).toEqual(["line 1", "line 2"]);
        expect(viewer.state.error).toBe(null);
        expect(viewer.state.loading).toBe(false);
    });

    test("a newer search is not overwritten by an older search that finishes later", async () => {
        // Tests [@ANCHOR: pager_duty:js_log_viewer_execute_search]
        const polls = { j1: 0, j2: 0 };
        let started = 0;
        mockFetch((url, init) => {
            const params = JSON.parse(init.body).params;
            if (url === "/api/v1/pager/logs/search") {
                started += 1;
                return { result: { job_id: `j${started}` } };
            }
            polls[params.job_id] += 1;
            if (params.job_id === "j1") {
                return { result: polls.j1 < 3 ? { status: "running" } : { status: "done", matches: ["old"] } };
            }
            return { result: { status: "done", matches: ["new"] } };
        });
        const viewer = bareViewer();
        const first = viewer.executeSearch();
        await animationFrame();
        viewer.state.regexQuery = "WARN";
        const second = viewer.executeSearch();
        await finish(Promise.all([first, second]));
        expect(viewer.state.results).toEqual(["new"]);
        expect(viewer.state.loading).toBe(false);
    });

    test("executeSearch reports no matches, a server error, an invalid reply and a failed request", async () => {
        // Tests [@ANCHOR: pager_duty:js_log_viewer_execute_search]
        stubApi({
            "/api/v1/pager/logs/search": [{ result: { job_id: "j" } }, { result: { error: "Bad pattern." } }, { result: {} }, { result: { job_id: "k" } }, new Response("not json")],
            "/api/v1/pager/logs/search_poll": [{ result: { status: "done", matches: [] } }, { result: { error: "Job lost." } }],
        });
        const none = bareViewer();
        const first = none.executeSearch();
        await finish(first);
        expect(none.state.error).toBe("0 matches found.");
        const refused = bareViewer();
        await refused.executeSearch();
        expect(refused.state.error).toBe("Bad pattern.");
        const invalid = bareViewer();
        await invalid.executeSearch();
        expect(invalid.state.error).toBe("Invalid API response.");
        const lost = bareViewer();
        const polled = lost.executeSearch();
        await finish(polled);
        expect(lost.state.error).toBe("Job lost.");
        const broken = bareViewer();
        await broken.executeSearch();
        expect(broken.state.error).toBe("IPC Request Failed.");
        expect(broken.state.loading).toBe(false);
    });

    test("executeSearch gives up with a timeout when the job never finishes", async () => {
        // Tests [@ANCHOR: pager_duty:js_log_viewer_execute_search]
        const replies = { "/api/v1/pager/logs/search": { result: { job_id: "j" } }, "/api/v1/pager/logs/search_poll": { result: { status: "running" } } };
        stubApi(replies);
        const viewer = bareViewer();
        const search = viewer.executeSearch();
        await finish(search);
        expect(viewer.state.error).toBe("Search timed out.");
    });
});
