/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_moderation_appeal]

// Tests [@ANCHOR: UX_SUBMIT_APPEAL]
registry.category("web_tour.tours").add("moderation_appeal_tour", {
    url: "/my/home",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        { trigger: 'body', content: 'Wait for: Verify Suspension Alert', run: function() {} },
        
    ],
});
