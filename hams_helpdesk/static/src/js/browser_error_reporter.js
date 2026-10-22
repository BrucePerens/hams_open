/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

// The browser half of the JavaScript error tickets (see controllers/browser_exception_api.py and
// docs/stories/server_exception_tickets.md, section 16). It listens for uncaught errors and unhandled
// promise rejections on pages of this site (website, portal and the back office) and posts a small,
// anonymous report to this site's own origin.
//
// What a report holds: the error class name, a message cut to 300 characters, up to 8 stack frames
// reduced to `url:line:col` with no query string, the page's route with every variable part replaced,
// the browser family and the asset bundle version. It never holds form values, page text, a URL with a
// query string, a cookie or anything about the user, and the request is sent without cookies.
//
// This file has no side effects when imported. browser_error_reporter_boot.js installs it on the page,
// so the unit tests can load this one without listening to the test runner's own errors.
//
// Deliberately not loaded by the relay-served console (shack_console / hams_local_relay): that is a
// local application and reporting from it needs its own opt-in privacy decision.

export const ENDPOINT = "/hams_helpdesk/browser_exception";
export const SCHEMA_VERSION = 1;
export const MAX_REPORTS_PER_PAGE = 5;
export const MIN_INTERVAL_MS = 1000;
export const MAX_FRAMES = 8;
export const MESSAGE_LIMIT = 300;
export const ROUTE_LIMIT = 200;
export const SEGMENT_LIMIT = 24;
export const IGNORED_TYPES = new Set([
    "RPCError",
    "RPC_ERROR",
    "ConnectionLostError",
    "ConnectionAbortedError",
    "AbortError",
    "NetworkError",
    "TimeoutError",
]);
export const IGNORED_MESSAGE_RE =
    /(ResizeObserver loop|^script error\.?$|failed to fetch|load failed|networkerror when attempting|the operation was aborted|the user aborted a request|network request failed|fetch is aborted)/i;

const TYPE_RE = /^[A-Za-z][A-Za-z0-9_$]{0,39}$/;
const FRAME_RE = /^(.*):(\d+):(\d+)$/;
const ASSET_RE = /\/web\/assets\/(?:\d+\/)?([0-9a-f]{7,64})\/([A-Za-z0-9_.]+)\.min\.js$/;

// [@ANCHOR: hams_helpdesk:js_strip_query]
export function stripQuery(url) {
    return String(url || "").split("?")[0].split("#")[0];
}

// [@ANCHOR: hams_helpdesk:js_reduce_stack]
// Frames of a Chrome (`at fn (url:1:2)`, `at url:1:2`) or Firefox/Safari (`fn@url:1:2`) stack as
// `url:line:col`, query strings and fragments removed, at most `limit` of them. Lines that are not
// frames (the message, `at <anonymous>`, `[native code]`) are dropped.
export function reduceStack(stack, limit = MAX_FRAMES) {
    const frames = [];
    for (const raw of String(stack || "").split("\n")) {
        let line = raw.trim();
        if (line.startsWith("at ")) {
            const open = line.lastIndexOf("(");
            line = open >= 0 && line.endsWith(")") ? line.slice(open + 1, -1) : line.slice(3);
        } else if (line.includes("@")) {
            line = line.slice(line.lastIndexOf("@") + 1);
        } else {
            continue;
        }
        const match = FRAME_RE.exec(line);
        if (!match) {
            continue;
        }
        frames.push(`${stripQuery(match[1])}:${match[2]}:${match[3]}`);
        if (frames.length >= limit) {
            break;
        }
    }
    return frames;
}

// [@ANCHOR: hams_helpdesk:js_normalize_route]
// The page path with every segment that holds a digit, or is long, replaced by `<x>`; no query string.
export function normalizeRoute(pathname) {
    const path = stripQuery(pathname) || "/";
    const segments = path.split("/").map((segment) =>
        /\d/.test(segment) || segment.length > SEGMENT_LIMIT ? "<x>" : segment
    );
    return segments.join("/").slice(0, ROUTE_LIMIT) || "/";
}

// [@ANCHOR: hams_helpdesk:js_agent_family]
// Only the browser's family, from a fixed list, never the user-agent string.
export function agentFamily(userAgent) {
    const ua = String(userAgent || "");
    if (/Edg(e|A|iOS)?\//.test(ua)) {
        return "Edge";
    }
    if (/OPR\/|Opera/.test(ua)) {
        return "Opera";
    }
    if (/Firefox\/|FxiOS\//.test(ua)) {
        return "Firefox";
    }
    if (/Chrome\/|CriOS\//.test(ua)) {
        return "Chrome";
    }
    if (/Safari\//.test(ua)) {
        return "Safari";
    }
    return "Other";
}

// [@ANCHOR: hams_helpdesk:js_release_of]
// `bundle@version` of the asset bundle: from the failing frame when it is a bundle file, else from the
// first bundle script the page loaded. Null when there is none (a debug=assets page), and then nothing
// is reported: the server only accepts the current bundle version.
export function releaseOf(topFrame, scriptUrls) {
    for (const url of [stripQuery(String(topFrame || "").replace(FRAME_RE, "$1")), ...(scriptUrls || [])]) {
        const match = ASSET_RE.exec(stripQuery(url));
        if (match) {
            return `${match[2]}@${match[1]}`;
        }
    }
    return null;
}

// [@ANCHOR: hams_helpdesk:js_is_ignorable]
// Errors that are not bugs in the page (see the server's IGNORED_TYPES): Odoo's RPC errors, which the
// server has already ticketed, a dropped connection, an aborted request, ResizeObserver notices.
export function isIgnorable(typeName, message, error) {
    if (IGNORED_TYPES.has(typeName) || IGNORED_MESSAGE_RE.test(message || "")) {
        return true;
    }
    return Boolean(error && typeof error === "object" && (error.exceptionName || (error.data && error.data.name)));
}

// [@ANCHOR: hams_helpdesk:js_build_report]
// The report object for one error, or null when nothing worth sending can be said about it.
// `error` is the thrown value; `fallback` is `{filename, lineno, colno}` from an `error` event, used
// when the value has no stack. `env` is `{origin, pathname, userAgent, scriptUrls}`.
export function buildReport(error, fallback, env) {
    const isObject = error !== null && typeof error === "object";
    const rawName = isObject ? error.name || (error.constructor && error.constructor.name) : "";
    const typeName = TYPE_RE.test(rawName || "") ? rawName : isObject ? "Error" : "UnhandledRejection";
    const rawMessage = isObject ? error.message : typeof error === "string" ? error : "";
    const message = stripUrlQueries(String(rawMessage || "")).slice(0, MESSAGE_LIMIT);
    if (isIgnorable(typeName, message, isObject ? error : null)) {
        return null;
    }
    let frames = isObject ? reduceStack(error.stack) : [];
    if (!frames.length && fallback && fallback.filename) {
        frames = [`${stripQuery(fallback.filename)}:${fallback.lineno || 0}:${fallback.colno || 0}`];
    }
    // The top frame must be one of this site's own scripts: an extension's or another site's error is
    // not ours to report.
    if (!frames.length || !frames[0].startsWith(env.origin + "/")) {
        return null;
    }
    const release = releaseOf(frames[0], env.scriptUrls);
    if (!release) {
        return null;
    }
    return {
        v: SCHEMA_VERSION,
        type: typeName,
        message,
        frames,
        route: normalizeRoute(env.pathname),
        agent: agentFamily(env.userAgent),
        release,
    };
}

// [@ANCHOR: hams_helpdesk:js_strip_url_queries]
// A message can quote a URL (`Failed to load https://x/y?token=...`); cut every query and fragment.
export function stripUrlQueries(text) {
    return text.replace(/(https?:\/\/[^\s?#'")]+)[?#][^\s'")]*/g, "$1");
}

// [@ANCHOR: hams_helpdesk:js_send_report]
// Posts the report with no cookies. A failure to send is ignored: reporting an error must never raise one.
export function sendReport(report, fetchImpl = globalThis.fetch && globalThis.fetch.bind(globalThis)) {
    if (!fetchImpl) {
        return null;
    }
    return fetchImpl(ENDPOINT, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(report),
        credentials: "omit",
        cache: "no-store",
        keepalive: true,
    }).catch(() => null);
}

export class BrowserErrorReporter {
    // [@ANCHOR: hams_helpdesk:js_reporter_construct]
    constructor(options = {}) {
        this.env = options.env;
        this.send = options.send || sendReport;
        this.now = options.now || (() => Date.now());
        this.seen = new Set();
        this.sent = 0;
        this.lastSent = -Infinity;
        this.busy = false;
    }

    // [@ANCHOR: hams_helpdesk:js_reporter_report]
    // Reports one error unless it was already reported on this page, the page has sent its five, or the last
    // one went less than a second ago. Never throws, and never re-enters itself.
    report(error, fallback) {
        if (this.busy) {
            return false;
        }
        this.busy = true;
        try {
            if (this.sent >= MAX_REPORTS_PER_PAGE) {
                return false;
            }
            const report = buildReport(error, fallback, this.currentEnv());
            if (!report) {
                return false;
            }
            const key = `${report.type}|${report.frames[0]}`;
            const when = this.now();
            if (this.seen.has(key) || when - this.lastSent < MIN_INTERVAL_MS) {
                return false;
            }
            this.seen.add(key);
            this.sent += 1;
            this.lastSent = when;
            this.send(report);
            return true;
        } catch {
            return false;
        } finally {
            this.busy = false;
        }
    }

    // [@ANCHOR: hams_helpdesk:js_reporter_env]
    currentEnv() {
        return typeof this.env === "function" ? this.env() : this.env;
    }

    // [@ANCHOR: hams_helpdesk:js_reporter_on_error]
    onError(event) {
        return this.report(event && event.error, event && {
            filename: event.filename,
            lineno: event.lineno,
            colno: event.colno,
        });
    }

    // [@ANCHOR: hams_helpdesk:js_reporter_on_rejection]
    onRejection(event) {
        return this.report(event && event.reason, null);
    }
}

// [@ANCHOR: hams_helpdesk:js_page_env]
export function pageEnv(win) {
    return {
        origin: win.location.origin,
        pathname: win.location.pathname,
        userAgent: win.navigator.userAgent,
        scriptUrls: Array.from(win.document.querySelectorAll("script[src]"), (script) => script.src),
    };
}

// [@ANCHOR: hams_helpdesk:js_install]
// Listens on `win` for uncaught errors and unhandled rejections. Odoo's own error service listens to the
// same two events to show its dialog; listeners do not hide each other's events, so both run.
export function install(win, options = {}) {
    const reporter = new BrowserErrorReporter({ env: () => pageEnv(win), ...options });
    win.addEventListener("error", (event) => reporter.onError(event));
    win.addEventListener("unhandledrejection", (event) => reporter.onRejection(event));
    return reporter;
}
