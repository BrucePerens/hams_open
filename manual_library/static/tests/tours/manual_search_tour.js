/** @odoo-module **/
import { registry } from "@web/core/registry";

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
            content: "Submit search",
            trigger: 'button[aria-label="Submit search"]',
            run: "click",
            expectUnloadPage: true,
        },
        {
            content: "Check results",
            trigger: '*:contains("Search Results for:")',
        }
    ],
});
