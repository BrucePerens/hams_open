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
                content: "[MACRO] Click the save button",
                trigger: saveButtonTrigger,
                run: 'click',
            },
            // Wait for the save resolution indicator
            this.waitForElement(waitTrigger, "RPC resolution / Dirty Form safe save (" + waitTrigger + ")")
        ];
    },

    /**
     * Safely clicks an element that uses the brittle `:contains(...)` selector without crashing
     * Odoo's native 'document.querySelectorAll' trigger evaluator.
     */
    clickElement: function (selector, description) {
        return {
            content: "[MACRO] Click element safely: " + (description || selector),
            trigger: 'body',
            run: function () {
                let selectors = selector.split(',').map(s => s.trim());
                for (let s of selectors) {
                    if (s.indexOf(':contains(') !== -1) {
                        let parts = s.split(':contains(');
                        let tag = parts[0] || '*';
                        let text = parts[1].replace(/['")]/g, '');
                        let elements = Array.prototype.slice.call(document.querySelectorAll(tag));
                        for (let i = 0; i < elements.length; i++) {
                            if (elements[i].textContent.indexOf(text) !== -1) {
                                elements[i].click();
                                return;
                            }
                        }
                    } else {
                        let el = document.querySelector(s);
                        if (el) {
                            el.click();
                            return;
                        }
                    }
                }
                throw new Error("Element not found to click: " + selector);
            }
        };
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
            trigger: 'body', // SAFE TRIGGER: Do not crash Odoo 19's native querySelector
            run: function () {
                let el = null;
                try {
                    el = document.querySelector(trigger);
                } catch(e) {}

                // Vanilla JS fallback for Odoo's native :contains jQuery selector
                if (!el && trigger.indexOf(':contains(') !== -1) {
                    let parts = trigger.split(':contains(');
                    let tag = parts[0] || '*';
                    let text = parts[1].replace(/['")]/g, '');
                    let elements = Array.prototype.slice.call(document.querySelectorAll(tag));
                    for (let i = 0; i < elements.length; i++) {
                        if (elements[i].textContent.indexOf(text) !== -1) {
                            el = elements[i];
                            break;
                        }
                    }
                }

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
                trigger: 'body', // SAFE TRIGGER: Removed :contains(...) to prevent Chrome DOMException
                run: function () {
                    const items = document.querySelectorAll('.o_select_menu_item');
                    for (let i = 0; i < items.length; i++) {
                        if (items[i].textContent.includes(itemText)) {
                            items[i].click();
                            return;
                        }
                    }
                    throw new Error("Could not find dropdown item: " + itemText);
                }
            }
        ];
    },

    /**
     * Pauses the tour until the network ledger is quiet, guaranteeing no pending backend operations
     * are mutating the DOM. Vital for severely constrained environments like the Jules VM.
     */
    waitForRPC: function () {
        return {
            content: "[MACRO] Wait for all pending RPCs to resolve (Jules VM Latency Protection)",
            trigger: 'body',
            run: function () {
                return new Promise(function (resolve, reject) {
                    let elapsed = 0;
                    const interval = setInterval(function () {
                        if (!window._pendingRPCCount || window._pendingRPCCount === 0) {
                            clearInterval(interval);
                            let overlay = document.getElementById('tour_rpc_overlay');
                            if (overlay) overlay.remove();
                            resolve();
                        } else {
                            elapsed++;
                            let msg = "[TourUtils] Jules VM Latency Shield | Elapsed: " + elapsed + "s | Waiting for " + window._pendingRPCCount + " pending network request(s)...";
                            console.log(msg);

                            let overlay = document.getElementById('tour_rpc_overlay');
                            if (!overlay) {
                                overlay = document.createElement('div');
                                overlay.id = 'tour_rpc_overlay';
                                overlay.style = 'position:fixed; top:10px; right:10px; z-index:999999; background:rgba(128,0,128,0.9); color:white; padding:15px; font-weight:bold; pointer-events:none; font-family:sans-serif; border-radius:5px;';
                                document.body.appendChild(overlay);
                            }
                            overlay.textContent = msg;

                            if (elapsed >= 120) {
                                clearInterval(interval);
                                const errorMsg = "FAILED: " + window._pendingRPCCount + " RPC requests failed to resolve after 120s!";
                                console.error(errorMsg);
                                if (overlay) overlay.remove();
                                reject(new Error(errorMsg));
                            }
                        }
                    }, 1000);
                });
            }
        };
    },

    /**
     * Pauses the tour until a specific DOM element appears and is visible.
     * Useful for bridging Owl asynchronous rendering gaps and intermittent timing failures.
     */
    waitForElement: function (trigger, description) {
        description = description || "";
        return {
            content: "[MACRO] Wait for DOM element: " + (description || trigger),
            trigger: 'body', // SAFE TRIGGER: Protects parser from SyntaxError timeouts
            run: function () {
                return new Promise(function (resolve, reject) {
                    let elapsed = 0;
                    const isFound = function () {
                        try {
                            if (document.querySelector(trigger)) return true;
                        } catch (e) {}

                        if (trigger.indexOf(':contains(') !== -1) {
                            let parts = trigger.split(':contains(');
                            let tag = parts[0] || '*';
                            let text = parts[1].replace(/['")]/g, '');
                            let elements = Array.prototype.slice.call(document.querySelectorAll(tag));
                            for (let i = 0; i < elements.length; i++) {
                                if (elements[i].textContent.indexOf(text) !== -1) return true;
                            }
                        }
                        return false;
                    };

                    if (isFound()) {
                        return resolve();
                    }

                    const interval = setInterval(function () {
                        elapsed++;
                        if (isFound()) {
                            clearInterval(interval);
                            let overlay = document.getElementById('tour_wait_overlay');
                            if (overlay) overlay.remove();
                            resolve();
                        } else {
                            let msg = "[TourUtils] Elapsed: " + elapsed + "s | Waiting for element: " + (description || trigger);
                            console.log(msg); // Log instead of error to bypass silent aborts

                            let overlay = document.getElementById('tour_wait_overlay');
                            if (!overlay) {
                                overlay = document.createElement('div');
                                overlay.id = 'tour_wait_overlay';
                                overlay.style = 'position:fixed; bottom:10px; right:10px; z-index:999999; background:rgba(255,0,0,0.9); color:white; padding:15px; font-weight:bold; pointer-events:none; font-family:sans-serif; border-radius:5px;';
                                document.body.appendChild(overlay);
                            }
                            overlay.textContent = msg;

                            if (elapsed >= 120) {
                                clearInterval(interval);
                                const errorMsg = "FAILED: Element not found after 120s: " + (description || trigger);
                                console.error(errorMsg);
                                if (overlay) overlay.remove();
                                reject(new Error(errorMsg));
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
            trigger: 'body', // SAFE TRIGGER
            run: function () {
                return new Promise(function (resolve, reject) {
                    let elapsed = 0;
                    const isAbsent = function () {
                        try {
                            if (document.querySelector(selector)) return false;
                        } catch (e) {}

                        if (selector.indexOf(':contains(') !== -1) {
                            let parts = selector.split(':contains(');
                            let tag = parts[0] || '*';
                            let text = parts[1].replace(/['")]/g, '');
                            let elements = Array.prototype.slice.call(document.querySelectorAll(tag));
                            for (let i = 0; i < elements.length; i++) {
                                if (elements[i].textContent.indexOf(text) !== -1) return false;
                            }
                        }
                        return true;
                    };

                    if (isAbsent()) {
                        return resolve();
                    }

                    const interval = setInterval(function () {
                        elapsed++;
                        if (isAbsent()) {
                            clearInterval(interval);
                            let overlay = document.getElementById('tour_absence_overlay');
                            if (overlay) overlay.remove();
                            resolve();
                        } else {
                            let msg = "[TourUtils] Elapsed: " + elapsed + "s | Waiting for absence of: " + (description || selector);
                            console.log(msg); // Log instead of error

                            let overlay = document.getElementById('tour_absence_overlay');
                            if (!overlay) {
                                overlay = document.createElement('div');
                                overlay.id = 'tour_absence_overlay';
                                overlay.style = 'position:fixed; bottom:10px; left:10px; z-index:999999; background:rgba(0,128,255,0.9); color:white; padding:15px; font-weight:bold; pointer-events:none; font-family:sans-serif; border-radius:5px;';
                                document.body.appendChild(overlay);
                            }
                            overlay.textContent = msg;

                            if (elapsed >= 120) {
                                clearInterval(interval);
                                const errorMsg = "FAILED: Element not removed after 120s: " + (description || selector);
                                console.error(errorMsg);
                                if (overlay) overlay.remove();
                                reject(new Error(errorMsg));
                            }
                        }
                    }, 1000);
                });
            },
        };
    }
};
