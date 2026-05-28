/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_frontend_misc]
registry.category("web_tour.tours").add("frontend_misc_tour", {
    url: "/user-websites/documentation",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        { trigger: 'body', content: 'Wait for: Verify Documentation Page renders correctly', run: function() {} },
        
    ],
});
