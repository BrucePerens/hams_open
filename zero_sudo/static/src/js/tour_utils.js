/** @odoo-module **/

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
            {
                content: "[MACRO] Wait for DOM element: RPC resolution / Dirty Form safe save (" + waitTrigger + ")",
                trigger: waitTrigger,
                run: function() {}
            }
        ];
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
                return new Promise((resolve) => {
                    const interval = setInterval(() => {
                        if (!document.querySelector(selector)) {
                            clearInterval(interval);
                            resolve();
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
                return new Promise((resolve) => {
                    const interval = setInterval(() => {
                        if (document.body.textContent.includes(text)) {
                            clearInterval(interval);
                            resolve();
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
                return new Promise((resolve) => {
                    const interval = setInterval(() => {
                        if (document.querySelector(selector)) {
                            clearInterval(interval);
                            resolve();
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
    }
};
