/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// Tests [@ANCHOR: story_manual_search]
// Tests [@ANCHOR: test_tour_manual_search]
// Tests [@ANCHOR: controller_manual_search]
registry.category("web_tour.tours").add("manual_search_tour", {
    url: "/manual",
    steps: () => [
        {
            content: "Enter search term",
            trigger: 'input[name="search"]',
            run: 'edit Odoo'
        },
        {
            content: 'Submit form and trigger page reload',
            trigger: 'button[aria-label="Submit search"]',
            run: 'click',
            expectUnloadPage: true,
        },
        { trigger: 'body', content: 'Wait for page reload', run: function() {} },
        {
            trigger: '#wrap',
            content: 'Check results',
            run: function () {
                if (!document.body.textContent.includes("Search Results for:")) {
                    throw new Error('Search results missing');
                }
            }
        },
    ],
});
