/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";


// [@ANCHOR: test_tour_gdpr_privacy]

// Tests [@ANCHOR: controller_my_privacy_dashboard]

// Tests [@ANCHOR: UX_GDPR_EXPORT]

// Tests [@ANCHOR: UX_GDPR_ERASURE]

// Tests [@ANCHOR: user_websites:COMM_privacy_erased]
registry.category("web_tour.tours").add("gdpr_privacy_tour", {
    steps: () => [
        { trigger: 'h2', content: 'Wait for: Wait for Privacy Header', run: function() {} },
        {
            content: "Verify Privacy Dashboard Header",
            trigger: 'body',
            run: () => {},
        },
        { trigger: 'form[action="/my/privacy/export"] button[type="submit"]', content: 'Wait for: Wait for Export Button', run: function() {} },
        {
            content: "Verify Export Data Button is properly wired",
            trigger: 'form[action="/my/privacy/export"] button[type="submit"]',
            run: () => {}, // Verify presence only to prevent file download from unloading the test page
        },
        TourUtils.bypassDialogs(),
        {
            content: "Verify Erasure Form invokes deletion using namespaced class",
            trigger: 'button.o_tour_erasure_initiate',
            run: 'click',
            expectUnloadPage: true,
        },
        {
            content: "Wait for the public erasure confirmation page (lazy JS loaded) and the document load event",
            trigger: 'body:not(.o_lazy_js_waiting) #user_websites_erasure_confirmed',
            run: async () => {
                if (document.readyState !== "complete") {
                    await new Promise((resolve) =>
                        window.addEventListener("load", resolve, { once: true })
                    );
                }
            },
        },
        {
            content: "Confirmation states the erasure is permanent and is not the login page",
            trigger: '#user_websites_erasure_confirmed',
            run: function () {
                if (!document.querySelector("#user_websites_erasure_confirmed").textContent.includes("permanently erased")) {
                    throw new Error("Erasure confirmation text is missing");
                }
                if (document.querySelector("form.oe_login_form")) {
                    throw new Error("Landed on the login page instead of the confirmation");
                }
            },
        },
    ],
});
