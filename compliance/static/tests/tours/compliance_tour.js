/** @odoo-module **/
import { registry } from "@web/core/registry";

// # Verified by [@ANCHOR: test_compliance_ui_tour]
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/privacy?debug=1",
    steps: () => [
        {
            content: "Verify Privacy Policy content",
            trigger: "#wrap:contains('Privacy Policy')",
            run: () => {},
        },
        {
            content: "Verify Warning message presence",
            trigger: "#wrap:contains('Warning: This is the default version')",
            run: () => {},
        },
        {
            content: "Verify Data Minimization section",
            trigger: "#wrap:contains('Data Minimization')",
            run: () => {},
        },
        {
            content: "Verify related links are present",
            trigger: "#wrap:contains('Related')",
            run: () => {},
        },
        {
            content: "Verify Cookie Policy link in related section",
            trigger: "a[href='/cookie-policy']",
            run: () => {},
        },
        {
            content: "Verify Terms of Service link in related section",
            trigger: "a[href='/terms']",
            run: () => {},
        }
    ],
});
