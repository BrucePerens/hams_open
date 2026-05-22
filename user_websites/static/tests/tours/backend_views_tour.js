/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// [@ANCHOR: test_tour_backend_views]
registry.category("web_tour.tours").add("backend_views_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        TourUtils.waitForElement('.o_main_navbar', 'Verify backend UI loads'),
        TourUtils.waitForRPC()
    ],
});
