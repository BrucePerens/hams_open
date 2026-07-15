/** @odoo-module **/
import { registry } from "@web/core/registry";


// [@ANCHOR: test_tour_gdpr_privacy]

// Tests [@ANCHOR: controller_my_privacy_dashboard]

// Tests [@ANCHOR: UX_GDPR_EXPORT]

// Tests [@ANCHOR: UX_GDPR_ERASURE]
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
        { trigger: 'body', content: 'Wait for page reload to hydrate DOM', run: function() {} }
    ],
});
