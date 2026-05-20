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
    safeSave(saveButtonTrigger = '.o_form_button_save', waitTrigger = '.o_form_button_create') {
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
            {
                content: "[MACRO] Wait for RPC resolution to prevent Dirty Form crashes",
                trigger: waitTrigger,
                run: () => {},
            }
        ];
    },

    /**
     * Silently intercepts and bypasses native blocking dialogs which would otherwise
     * permanently halt the headless browser thread.
     */
    bypassDialogs() {
        return {
            content: "[MACRO] Bypass native blocking dialogs",
            trigger: 'body',
            run: () => {
                window.alert = () => {};
                window.confirm = () => true;
            }
        };
    },

    /**
     * Injects an interceptor to mock external frontend XHR/fetch requests
     * (e.g., Turnstile, external APIs) to prevent network-latency induced flappiness.
     */
    mockExternalRequests(urlPattern, mockResponse) {
        return {
            content: `[MACRO] Mock external requests for ${urlPattern}`,
            trigger: 'body',
            run: () => {
                const originalFetch = window.fetch;
                window.fetch = async (...args) => {
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
    deterministicInput(trigger, value) {
        return {
            content: `[MACRO] Deterministic input for ${trigger}`,
            trigger: trigger,
            run: () => {
                const el = document.querySelector(trigger);
                if (!el) {
                    throw new Error(`[FATAL] Element not found for deterministic input: ${trigger}`);
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
    clickAndUnload(trigger) {
        return {
            content: `[MACRO] Click and expect page unload: ${trigger}`,
            trigger: trigger,
            run: 'click',
            expectUnloadPage: true,
        };
    },

    /**
     * Odoo 19 removed native <select> elements in backend views.
     * This macro executes the two-step click sequence for the new .o_select_menu.
     */
    selectDropdown(dropdownTrigger, itemText) {
        return [
            {
                content: `[MACRO] Open select menu: ${dropdownTrigger}`,
                trigger: dropdownTrigger,
                run: 'click',
            },
            {
                content: `[MACRO] Select menu item: ${itemText}`,
                trigger: `.o_select_menu_item:contains("${itemText}")`,
                run: 'click',
            }
        ];
    },

    /**
     * Pauses the tour until a specific DOM element appears and is visible.
     * Useful for bridging Owl asynchronous rendering gaps and intermittent timing failures.
     */
    waitForElement(trigger, description = "") {
        return {
            content: `[MACRO] Wait for DOM element: ${description || trigger}`,
            trigger: 'body',
            run: async () => {
                return new Promise((resolve) => {
                    let elapsed = 0;
                    const isFound = () => !!document.querySelector(trigger);

                    if (isFound()) {
                        return resolve();
                    }

                    const interval = setInterval(() => {
                        elapsed++;
                        if (isFound()) {
                            clearInterval(interval);
                            resolve();
                        } else {
                            console.log(`[TourUtils] Waiting... Script: Active UI Tour | Elapsed: ${elapsed}s | Waiting for element: ${description || trigger}`);
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
    waitForAbsence(selector, description = "") {
        return {
            content: `[MACRO] Wait for DOM absence: ${description || selector}`,
            trigger: 'body',
            run: async () => {
                return new Promise((resolve) => {
                    let elapsed = 0;
                    const isAbsent = () => !document.querySelector(selector);

                    if (isAbsent()) {
                        return resolve();
                    }

                    const interval = setInterval(() => {
                        elapsed++;
                        if (isAbsent()) {
                            clearInterval(interval);
                            resolve();
                        } else {
                            console.log(`[TourUtils] Waiting... Script: Active UI Tour | Elapsed: ${elapsed}s | Waiting for absence of: ${description || selector}`);
                        }
                    }, 1000);
                });
            },
        };
    }
};
