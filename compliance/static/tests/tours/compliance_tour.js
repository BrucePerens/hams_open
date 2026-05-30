/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// # Verified by [@ANCHOR: test_compliance_ui_tour]
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/privacy?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        { trigger: '#wrap', content: 'Wait for wrap' },
        {
            trigger: 'h1',
            content: 'Verify Privacy Policy content',
            run: function () {
                const text = document.body.textContent;
                // Check shortened to avoid HTML newline characters breaking the match
                if (!text.includes('Privacy Policy') || !text.includes('Warning: This is the default') || !text.includes('Data Minimization') || !text.includes('Related')) {
                    throw new Error('Compliance content missing');
                }
            }
        },
        {
            trigger: "a[href='/cookie-policy']",
            content: 'Verify Cookie Policy link in related section',
        },
        {
            trigger: "a[href='/terms']",
            content: 'Verify Terms of Service link in related section',
        },
    ],
});
