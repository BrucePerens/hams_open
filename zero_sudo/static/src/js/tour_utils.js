/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

/**
 * Centralized macros for Odoo UI Tours to guarantee architectural compliance.
 * Refactored to eliminate MutationObserver layout thrashing and recursive fetch wrappers.
 */
export const TourUtils = {
    safeSave: function (saveButtonTrigger, waitTrigger) {
        saveButtonTrigger = saveButtonTrigger || '.o_form_button_save';
        waitTrigger = waitTrigger || '.o_form_button_create';
        return [
            {
                content: "[MACRO] Click the save button",
                trigger: saveButtonTrigger,
                run: 'click',
            },
            TourUtils.waitForElement(waitTrigger, "RPC resolution / Dirty Form safe save")
        ];
    },

    dismissCookiesBar: function () {
        return {
            content: "[MACRO] Remove the website cookies bar before it can auto-show and out-rank a real modal in Odoo's tour engine's \"last visible modal\" check",
            trigger: 'body',
            run: function () {
                // #website_cookies_bar (website/views/website_templates.xml)
                // auto-shows ~500ms after page load and is itself a
                // Bootstrap .modal, injected into website.layout. web_tour's
                // own elementIsInModal safety check (tour_step_automatic.js)
                // picks the LAST visible .modal in DOM order and refuses any
                // click/wait outside that one -- so once the cookies bar
                // appears, every other tour step fails with "It is not
                // allowed to do action on an element that's below a modal",
                // even on a page with no modal of its own (see
                // docs/proposals/EVENT_ISSUE_TOUR_MODAL_FLAKE.md for the
                // original diagnosis and ADR 0081 rule 10 for the general
                // writeup). Extracted here after it was independently
                // rediscovered on ham_propagation's and ham_testing's own
                // tours, which had no cookies-bar handling at all.
                document.querySelector('#website_cookies_bar')?.remove();

                // The above alone is NOT sufficient: #website_cookies_bar's
                // *actual* Bootstrap modal is a nested, id-less
                // `.modal.o_cookies_discrete` child that something in its
                // own init (Bootstrap's Modal, or Odoo's Popup/CookiesBar
                // interaction wrapping it) reparents out to a direct child
                // of <body>, independent of its wrapper, timed non-
                // deterministically relative to this step. Once reparented,
                // removing the (now-vacated) wrapper does nothing for it.
                // Scoped narrowly to this one, specifically-identified
                // element -- NOT "every visible modal" -- matching this
                // exact concern from the same investigation: an earlier,
                // broader "hide every visible modal on any mutation"
                // approach was tried elsewhere in this codebase and
                // reverted for being too aggressive (it would hide
                // legitimate modals mid-tour). This one only ever matches
                // the cookies bar's own reparented node.
                new MutationObserver((mutations) => {
                    for (const m of mutations) {
                        for (const node of m.addedNodes) {
                            if (
                                node.nodeType === 1 &&
                                node.classList?.contains('modal') &&
                                node.classList?.contains('o_cookies_discrete')
                            ) {
                                node.remove();
                            }
                        }
                    }
                }).observe(document.body, { childList: true });
            },
        };
    },

    bypassDialogs: function () {
        return {
            content: "[MACRO] Bypass native blocking dialogs",
            trigger: 'body',
            run: function () {
                if (!window.__dialogsBypassed) {
                    window.alert = function (msg) {
                        console.warn("[ALARM] Native window.alert intercepted! Message: " + msg);
                    };
                    window.confirm = function (msg) {
                        console.warn("[ALARM] Native window.confirm intercepted! Message: " + msg);
                        return true;
                    };
                    window.__dialogsBypassed = true;
                }
            }
        };
    },

    mockExternalRequests: function (urlPattern, mockResponse) {
        return {
            content: "[MACRO] Mock external requests for " + urlPattern,
            trigger: 'body',
            run: function () {
                if (!window.__originalFetch) {
                    window.__originalFetch = window.fetch;
                    window.__mockResponses = {};
                    window.fetch = async function (...args) {
                        const url = typeof args[0] === 'string' ? args[0] : (args[0] ? args[0].url : '');
                        for (const [pattern, response] of Object.entries(window.__mockResponses)) {
                            if (url.includes(pattern)) {
                                return new Response(JSON.stringify(response), { status: 200 });
                            }
                        }
                        return window.__originalFetch.apply(this, args);
                    };
                }
                window.__mockResponses[urlPattern] = mockResponse;
            }
        };
    },

    waitForAbsence: function (selector, description) {
        description = description || "";
        return {
            content: "[MACRO] Wait for DOM absence: " + (description || selector),
            trigger: 'body',
            run: function () {
                return new Promise((resolve, reject) => {
                    let elapsed = 0;
                    const interval = setInterval(() => {
                        elapsed += 250;
                        if (!document.querySelector(selector)) {
                            clearInterval(interval);
                            resolve();
                        } else if (elapsed >= 10000) {
                            clearInterval(interval);
                            reject(new Error("Timeout waiting for absence of element: " + selector));
                        }
                    }, 250);
                });
            }
        };
    },

    waitForText: function (text, description) {
        description = description || "";
        return {
            content: "[MACRO] Wait for DOM text: " + (description || text),
            trigger: 'body',
            run: function () {
                return new Promise((resolve, reject) => {
                    let elapsed = 0;
                    const interval = setInterval(() => {
                        elapsed += 250;
                        if (document.body.textContent.includes(text)) {
                            clearInterval(interval);
                            resolve();
                        } else if (elapsed >= 10000) {
                            clearInterval(interval);
                            reject(new Error("Timeout waiting for text: " + text));
                        }
                    }, 250);
                });
            }
        };
    },

    waitForElement: function (selector, description) {
        description = description || "";
        return {
            content: "[MACRO] Wait for DOM element: " + (description || selector),
            trigger: 'body',
            run: function () {
                return new Promise((resolve, reject) => {
                    let elapsed = 0;
                    const interval = setInterval(() => {
                        elapsed += 250;
                        if (document.querySelector(selector)) {
                            clearInterval(interval);
                            resolve();
                        } else if (elapsed >= 10000) {
                            clearInterval(interval);
                            reject(new Error("Timeout waiting for element: " + selector));
                        }
                    }, 250);
                });
            }
        };
    },

    deterministicInput: function (helpers, text) {
        // Find the active element (typically focused by the previous 'click' step)
        const el = document.activeElement;
        if (!el || (el.tagName !== 'INPUT' && el.tagName !== 'TEXTAREA')) {
            console.warn("[MACRO] deterministicInput: Active element is not an input or textarea.");
            return;
        }

        // Safely inject text and explicitly fire the events required by Odoo's autocomplete widgets
        el.value = text;
        el.dispatchEvent(new Event('input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));

        // Fire keyup to trigger the Owl/Many2one search debouncer
        const keyUpEvent = new KeyboardEvent('keyup', {
            bubbles: true,
            key: text.slice(-1),
            code: 'Key' + text.slice(-1).toUpperCase()
        });
        el.dispatchEvent(keyUpEvent);
    },

    /**
     * Not a step factory like the macros above -- call this directly from
     * inside a tour step's `run: async function() {...}` body:
     * `await TourUtils.assertSettles(somePromise(), 2000, "enforceLRUQuota")`.
     *
     * A Promise that's supposed to reject on error but has a missing
     * onerror/onabort handler somewhere in an IndexedDB request chain just
     * hangs forever instead -- and a hung tour step times out with a
     * generic "trigger not found" message, giving no hint that IndexedDB
     * was the cause (see caching/static/src/sw/sw.js's enforceLRUQuota()
     * and ham_shack/static/src/sw/shack_sw.js's flushOfflineLogs(), both
     * of which had exactly this bug). Wrapping the promise under test in
     * this turns that silent, generic timeout into an immediate, specific
     * failure naming which operation didn't settle.
     */
    assertSettles: function (promise, timeoutMs, label) {
        label = label || "operation";
        let timeoutId;
        const timeout = new Promise((_resolve, reject) => {
            timeoutId = setTimeout(() => {
                reject(new Error(`[assertSettles] "${label}" did not settle within ${timeoutMs}ms -- likely a missing onerror/onabort handler leaving a Promise permanently unsettled.`));
            }, timeoutMs);
        });
        return Promise.race([promise, timeout]).finally(() => clearTimeout(timeoutId));
    },

    /**
     * Not a step factory -- call directly from a step's `run:` body:
     * `const shack = TourUtils.findOwlComponent((c) => typeof c.processTranscript === "function");`
     *
     * The only way a tour can exercise UI whose real trigger is an
     * external event a tour can't synthesize (e.g. the browser's own
     * SpeechRecognition API delivering a result -- see
     * HAM_SHACK_SPEECH_RECOGNITION.md) is to reach the live OWL component
     * instance directly and call its handler method as if the event had
     * fired. window.__OWL_DEVTOOLS__.apps is OWL's own registry of every
     * live App in the page (owl.js exposes it unconditionally, not just in
     * dev mode); each App's root ComponentNode links down through
     * .children (a plain object keyed by an internal parentKey, not an
     * array) to every mounted component's real instance via .component.
     * Confirmed against this Odoo version's actual bundled owl.js source
     * (web/static/lib/owl/owl.js) rather than assumed -- OWL's internals
     * are undocumented/unstable API, and no precedent for this technique
     * existed anywhere in this codebase before this helper. Real bug
     * found and fixed 2026-09-01, once the OOM-watchdog memory ceiling
     * (hams_shared commit c8f8735) and a missing web.assets_tests
     * manifest entry (ham_shack's voice_command_help_tour.js was written
     * but never wired into the bundle at all -- see night_shift_todo.md's
     * write-up of that same night) had both been separately fixed,
     * finally letting this function actually run against a live page for
     * the first time: the page's real content App
     * (as opposed to Odoo's own MainComponentsContainer utility App) does
     * NOT mount its content on `app.root` at all in this Odoo/OWL version
     * -- confirmed via a temporary diagnostic dump of a live App's own
     * property keys and values, not assumed. `app.root` was `undefined`
     * while `app.subRoots` (a `Map` from an internal numeric key to a
     * `ComponentNode`) held the real content root -- WebShack itself,
     * for `/shack`. Walking only `app.root`, as this function originally
     * did, silently found nothing on every real page, which is exactly
     * why this had "never successfully completed a live tour run end to
     * end" -- the technique's core traversal was wrong, not the specific
     * caller. Fixed by also walking every `app.subRoots` entry.
     *
     * Takes a predicate, not a class name: assets can be served minified
     * (assetsbundle.py's is_debug_assets gate, independent of plain
     * ?debug=1) and a minifier mangles top-level class *names* by default
     * -- `constructor.name === "WebShack"` would silently stop matching
     * under a minified bundle. Method names defined in a class body are
     * ordinary object properties, which default minifier config does not
     * mangle, so a predicate that duck-types on a real, distinctive method
     * survives minification where a class-name check would not.
     */
    findOwlComponent: function (predicate) {
        if (typeof window.__OWL_DEVTOOLS__ === "undefined") {
            throw new Error("[MACRO] findOwlComponent: window.__OWL_DEVTOOLS__ is not present -- this OWL build may no longer expose it, or nothing has mounted yet.");
        }
        function walk(node) {
            if (!node) return null;
            if (node.component && predicate(node.component)) {
                return node.component;
            }
            for (const key in node.children) {
                const found = walk(node.children[key]);
                if (found) return found;
            }
            return null;
        }
        for (const app of window.__OWL_DEVTOOLS__.apps) {
            let found = walk(app.root);
            if (found) return found;
            // See this function's own doc comment: a page's real content
            // App mounts its content here, not on app.root, in this
            // Odoo/OWL version.
            if (app.subRoots) {
                for (const subRoot of app.subRoots.values()) {
                    found = walk(subRoot);
                    if (found) return found;
                }
            }
        }
        return null;
    }
};
