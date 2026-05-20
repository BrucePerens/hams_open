/** @odoo-module **/

/**
 * Centralized macros for Odoo UI Tours to guarantee architectural compliance
 * and reduce boilerplate when navigating "Dirty Form" and DOM race conditions.
 */
export const TourUtils = {
    /**
     * Executes the mandated safe-save sequence for forms with `type="object"` buttons.
     * Forces a DOM blur, clicks save, and explicitly waits for the RPC to resolve.
     */
    safeSave: function (saveButtonTrigger, waitTrigger) {
        saveButtonTrigger = saveButtonTrigger || '.o_form_button_save';
        waitTrigger = waitTrigger || '.o_form_button_create';
        return [
            {
                content: "[MACRO] Click away to force DOM blur and commit pending text inputs",
                trigger: '.o_form_sheet',
                run: 'click',
            },
            {
                content: "[MACRO] Click the save button",
                trigger: saveButtonTrigger,
                run: 'click',
            },
            this.waitForElement(waitTrigger, "RPC resolution / Dirty Form safe save (" + waitTrigger + ")")
        ];
    },

    /**
     * Silently intercepts and bypasses native blocking dialogs which would otherwise
     * permanently halt the headless browser thread. Alarms are raised upon interception.
     */
    bypassDialogs: function () {
        return {
            content: "[MACRO] Bypass native blocking dialogs",
            trigger: 'body',
            run: function () {
                window.alert = function (msg) {
                    console.error("[ALARM] Native window.alert intercepted and bypassed! Message: " + msg);
                };
                window.confirm = function (msg) {
                    console.error("[ALARM] Native window.confirm intercepted and bypassed! Message: " + msg);
                    return true;
                };
            }
        };
    },

    /**
     * Injects an interceptor to mock external frontend XHR/fetch requests
     * (e.g., Turnstile, external APIs) to prevent network-latency induced flappiness.
     */
    mockExternalRequests: function (urlPattern, mockResponse) {
        return {
            content: "[MACRO] Mock external requests for " + urlPattern,
            trigger: 'body',
            run: function () {
                const originalFetch = window.fetch;
                window.fetch = async function () {
                    const args = Array.prototype.slice.call(arguments);
                    const url = typeof args[0] === 'string' ? args[0] : (args[0] ? args[0].url : '');
                    if (url.includes(urlPattern)) {
                        return new Response(JSON.stringify(mockResponse), { status: 200 });
                    }
                    return originalFetch.apply(this, args);
                };
            }
        };
    },

    /**
     * Safely inputs text bypassing the 'edit' helper's character-by-character simulation.
     * Prevents race conditions on strict validation fields (Enforces ADR-0081 Section 9).
     */
    deterministicInput: function (trigger, value) {
        return {
            content: "[MACRO] Deterministic input for " + trigger,
            trigger: trigger,
            run: function () {
                const el = document.querySelector(trigger);
                if (!el) {
                    throw new Error("[FATAL] Element not found for deterministic input: " + trigger);
                }
                el.value = value;
                el.dispatchEvent(new Event('input', { bubbles: true }));
                el.dispatchEvent(new Event('change', { bubbles: true }));
            }
        };
    },

    /**
     * Executes a click that initiates a hard browser navigation or standard HTML form submit.
     * Warns the test runner to expect the unload event (Enforces ADR-0081 Section 3).
     */
    clickAndUnload: function (trigger) {
        return {
            content: "[MACRO] Click and expect page unload: " + trigger,
            trigger: trigger,
            run: 'click',
            expectUnloadPage: true,
        };
    },

    /**
     * Odoo 19 removed native <select> elements in backend views.
     * This macro executes the two-step click sequence for the new .o_select_menu.
     */
    selectDropdown: function (dropdownTrigger, itemText) {
        return [
            {
                content: "[MACRO] Open select menu: " + dropdownTrigger,
                trigger: dropdownTrigger,
                run: 'click',
            },
            {
                content: "[MACRO] Select menu item: " + itemText,
                trigger: '.o_select_menu_item:contains("' + itemText + '")',
                run: 'click',
            }
        ];
    },

    /**
     * Pauses the tour until a specific DOM element appears and is visible.
     * Useful for bridging Owl asynchronous rendering gaps and intermittent timing failures.
     */
    waitForElement: function (trigger, description) {
        description = description || "";
        return {
            content: "[MACRO] Wait for DOM element: " + (description || trigger),
            trigger: 'body',
            run: async function () {
                return new Promise(function (resolve) {
                    let elapsed = 0;
                    const isFound = function () { return !!document.querySelector(trigger); };

                    if (isFound()) {
                        return resolve();
                    }

                    const interval = setInterval(function () {
                        elapsed++;
                        if (isFound()) {
                            clearInterval(interval);
                            resolve();
                        } else {
                            console.error("[TourUtils] Waiting... Script: Active UI Tour | Elapsed: " + elapsed + "s | Waiting for element: " + (description || trigger));
                            if (elapsed >= 60) {
                                clearInterval(interval);
                                console.error("TIMEOUT: Element not found after 60s: " + (description || trigger));
                                resolve();
                            }
                        }
                    }, 1000);
                });
            },
        };
    },

    /**
     * Pauses the tour until a specific DOM element is completely removed from the document.
     * Useful for waiting for asynchronous modal closures, RPC blockers, or loading overlay removals.
     */
    waitForAbsence: function (selector, description) {
        description = description || "";
        return {
            content: "[MACRO] Wait for DOM absence: " + (description || selector),
            trigger: 'body',
            run: async function () {
                return new Promise(function (resolve) {
                    let elapsed = 0;
                    const isAbsent = function () { return !document.querySelector(selector); };

                    if (isAbsent()) {
                        return resolve();
                    }

                    const interval = setInterval(function () {
                        elapsed++;
                        if (isAbsent()) {
                            clearInterval(interval);
                            resolve();
                        } else {
                            console.error("[TourUtils] Waiting... Script: Active UI Tour | Elapsed: " + elapsed + "s | Waiting for absence of: " + (description || selector));
                            if (elapsed >= 60) {
                                clearInterval(interval);
                                console.error("TIMEOUT: Element not removed after 60s: " + (description || selector));
                                resolve();
                            }
                        }
                    }, 1000);
                });
            },
        };
    }
};
