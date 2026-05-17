/** @odoo-module **/
import { registry } from "@web/core/registry";

// # Verified by [@ANCHOR: test_compliance_ui_tour]
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/privacy?debug=1",
    steps: () => [
        {
            content: "Verify Privacy Policy content",
            trigger: "main *:contains('Privacy Policy')",
            run: () => {},
        },
        {
            content: "Verify related links are present",
            trigger: "main *:contains('Related')",
            run: () => {},
        },
        {
            content: "Verify Cookie Policy link in related section",
            trigger: "main a[href='/cookie-policy']",
            run: () => {},
        },
        {
            content: "Verify Terms of Service link in related section",
            trigger: "main a[href='/terms']",
            run: () => {},
        }
    ],
});
