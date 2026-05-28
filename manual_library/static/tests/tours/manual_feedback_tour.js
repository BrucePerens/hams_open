/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Tests [@ANCHOR: story_manual_feedback]
// Tests [@ANCHOR: test_tour_manual_feedback]
// Tests [@ANCHOR: controller_manual_feedback]
registry.category("web_tour.tours").add("manual_feedback_tour", {
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: 'button[name="is_helpful"][value="1"]',
            content: 'Submit feedback and trigger reload',
            run: 'click',
            expectUnloadPage: true,
        },
        {
            trigger: 'body',
            content: 'Wait for page reload',
            run: function() {}
        },
        {
            trigger: '.alert-success',
            content: 'Check success message',
            run: function () {
                const els = document.querySelectorAll('.alert-success');
                let found = false;
                for (const el of els) {
                    if (el.textContent.includes('Thank you for your feedback!')) {
                        found = true;
                        break;
                    }
                }
                if (!found) {
                    throw new Error('Success message not found');
                }
            }
        },
    ],
});
