/** @odoo-module **/
import { registry } from "@web/core/registry";

// Tests [@ANCHOR: story_manual_feedback]
// Tests [@ANCHOR: test_tour_manual_feedback]
// Tests [@ANCHOR: controller_manual_feedback]
registry.category("web_tour.tours").add("manual_feedback_tour", {
    steps: () => [
        {
            content: "Click Helpful button",
            trigger: 'button[name="is_helpful"][value="1"]',
            run: "click",
            expectUnloadPage: true,
        },
        {
            content: "Check success message",
            trigger: '.alert-success:contains("Thank you for your feedback!")',
        }
    ],
});
