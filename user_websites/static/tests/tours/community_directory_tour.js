/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_community_directory]

// Tests [@ANCHOR: UX_COMMUNITY_DIRECTORY]
registry.category("web_tour.tours").add("community_directory_tour", {
    url: "/community",
    steps: () => [
        { trigger: 'h1', content: 'Wait for: Wait for Directory Header', run: function() {} },
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
