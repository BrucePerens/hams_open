/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Tests [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
registry.category("web_tour.tours").add("test_tour_violation_report", {
    url: "/",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        TourUtils.bypassDialogs(),
        {
            trigger: 'button[data-bs-target="#reportViolationModal"]',
            content: "Open violation reporting modal",
            run: "click",
        },
        {
            trigger: 'textarea[name="description"]',
            content: "Provide description notes",
            run: "edit Unsolicited advertising links.",
        },
        {
            trigger: 'button[type="submit"].btn-danger',
            content: "Submit violation ticket and trigger page reload",
            run: "click",
            expectUnloadPage: true,
        },
        {
            trigger: 'body',
            content: 'Wait for page reload after successful controller redirect',
            run: function() {}
        }
    ]
});
