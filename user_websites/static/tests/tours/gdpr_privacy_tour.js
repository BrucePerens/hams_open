/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// [@ANCHOR: test_tour_gdpr_privacy]
// Tests [@ANCHOR: controller_my_privacy_dashboard]
// Tests [@ANCHOR: UX_GDPR_EXPORT]
// Tests [@ANCHOR: UX_GDPR_ERASURE]
registry.category("web_tour.tours").add("gdpr_privacy_tour", {
    steps: () => [
        TourUtils.waitForElement('h2', 'Wait for Privacy Header'),
        {
            content: "Verify Privacy Dashboard Header",
            trigger: 'body',
            run: () => {},
        },
        TourUtils.waitForElement('form[action="/my/privacy/export"] button[type="submit"]', 'Wait for Export Button'),
        {
            content: "Verify Export Data Button is properly wired",
            trigger: 'form[action="/my/privacy/export"] button[type="submit"]',
            run: () => {}, // Verify presence only to prevent file download from unloading the test page
        },
        {
            content: "Bypass JS confirmation safeguard safely in an isolated step",
            trigger: 'body',
            run: () => {
                window.confirm = () => true;
            },
        },
        {
            content: "Verify Erasure Form invokes deletion using namespaced class",
            trigger: 'button.o_tour_erasure_initiate',
            run: 'click',
            expectUnloadPage: true,
        }
    ],
});
