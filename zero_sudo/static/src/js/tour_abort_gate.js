/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

// Pure helpers of the headless-browser tour watchdog (tour_failure_dump.js). They live apart from
// it because that file installs global handlers and replaces console.error when it loads, which
// a unit test must not do. Nothing here touches the page.

// [@ANCHOR: zero_sudo:js_describe_rejection_reason]
// A promise rejected with a DOM Event (a stylesheet or script load error, an IndexedDB or
// WebSocket error event) has no stack and prints as "[object Event]", which names nothing.
// Returns that text plus the event's type, constructor and, when it still has one, the target's
// tag, URL, readyState and error. Anything that is not an Event is returned unchanged.
export function describeRejectionReason(reason) {
    try {
        if (typeof Event !== "undefined" && reason instanceof Event) {
            const target = reason.target;
            const parts = [
                "Event type=" + reason.type,
                "constructor=" + (reason.constructor && reason.constructor.name),
            ];
            if (target) {
                parts.push("target=" + (target.tagName || (target.constructor && target.constructor.name) || String(target)));
                const where = target.currentSrc || target.src || target.href || target.url;
                if (where) {
                    parts.push("url=" + where);
                }
                if (target.error && target.error.name) {
                    parts.push("error=" + target.error.name + ": " + target.error.message);
                }
                if (target.readyState !== undefined) {
                    parts.push("readyState=" + target.readyState);
                }
            } else {
                parts.push("target=none (a shadow-root or detached node, or the page is closing)");
            }
            return "[object Event] " + parts.join(" ");
        }
    } catch (error) {
        return "[object Event] (undescribable: " + error + ")";
    }
    return reason;
}

// [@ANCHOR: zero_sudo:js_describe_phase]
// One line saying where the page is in its life when an alarm fires: how long ago the tour
// reported success (phase.succeededAt, milliseconds since the epoch, 0 for not yet), how long ago
// the page began to unload (phase.unloadingAt, same units), and the document's readiness and
// visibility. `now` is Date.now(); `doc` is the document.
export function describePhase(phase, now, doc) {
    return (
        "Phase: tour succeeded " +
        (phase.succeededAt ? now - phase.succeededAt + " ms ago" : "no") +
        "; unloading " +
        (phase.unloadingAt ? now - phase.unloadingAt + " ms ago" : "no") +
        "; readyState=" + doc.readyState +
        "; visibility=" + doc.visibilityState
    );
}

// [@ANCHOR: zero_sudo:js_abort_channel]
// Where an alarm goes. Before the tour has reported success an alarm is "fatal": console.error,
// which Odoo's browser harness turns into a failed test, so an error inside a tour always fails
// it. Once the tour has reported success (phase.succeededAt is set) the test has already passed
// and Python is closing the page: an alarm then is a page being torn down (a lazy stylesheet
// whose load is cut off rejects with its error Event; the final step's own page reload fires
// the tour engine's beforeunload check after the tour is gone). That alarm is "teardown":
// console.warn, logged with its full dump but not a failure.
export function abortChannel(phase) {
    return phase && phase.succeededAt ? "teardown" : "fatal";
}
