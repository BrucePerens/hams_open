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
    safeSave(saveButtonTrigger = '.o_form_button_save', waitTrigger = '.o_notification:contains("Success")') {
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
    }
};
