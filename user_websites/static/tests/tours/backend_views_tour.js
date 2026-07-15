/** @odoo-module **/
import { registry } from "@web/core/registry";


// Tests [@ANCHOR: test_tour_backend_views]
registry.category("web_tour.tours").add("backend_views_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        { trigger: '.o_main_navbar', content: 'Wait for: Verify backend UI loads', run: function() {} },
        
    ],
});
