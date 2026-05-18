/** @odoo-module **/
import { registry } from "@web/core/registry";

// [@ANCHOR: test_tour_backend_views]
registry.category("web_tour.tours").add("backend_views_tour", {
    url: "/odoo",
    steps: () => [
        {
            content: "Verify backend UI loads",
            trigger: '.o_main_navbar',
            run: () => {
                if (!document.querySelector('.o_main_navbar')) {
                    console.error("Backend navbar missing");
                }
            }
        }
    ],
});
