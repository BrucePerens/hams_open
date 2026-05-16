/** @odoo-module **/
import { registry } from "@web/core/registry";

// # Verified by [@ANCHOR: test_compliance_ui_tour]
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/",
    steps: () => [
        {
            content: "Click on Cookie Policy link in cookie bar",
            trigger: "#website_cookies_bar a[href='/cookie-policy']",
            run: "click",
            expectUnloadPage: true,
        },
        {
            content: "Verify Cookie Policy page title",
            trigger: "*:contains('Cookie Policy')",
            run: () => {},
        }
    ],
});
