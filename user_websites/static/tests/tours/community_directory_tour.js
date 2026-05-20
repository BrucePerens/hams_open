/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// [@ANCHOR: test_tour_community_directory]
// Tests [@ANCHOR: UX_COMMUNITY_DIRECTORY]
registry.category("web_tour.tours").add("community_directory_tour", {
    url: "/community",
    steps: () => [
        TourUtils.waitForElement('h1', 'Wait for Directory Header'),
        {
            content: "Check that the directory page renders",
            trigger: 'body',
            run: () => {
                if (!document.querySelector('h1').textContent.includes('Community Directory')) {
                    console.error("Directory header missing");
                }
            }
        }
    ],
});
