/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// [@ANCHOR: test_tour_create_blog]
// Tests [@ANCHOR: controller_user_blog_index]
// Tests [@ANCHOR: UX_CREATE_BLOG_POST]
registry.category("web_tour.tours").add("create_blog_tour", {
    url: "/blogtour/blog",
    steps: () => [
        TourUtils.waitForElement('button.o_tour_create_site_btn', 'Wait for Create Blog Button'),
        {
            content: "Click Create using namespaced fallback class",
            trigger: 'button.o_tour_create_site_btn',
            run: 'click',
            expectUnloadPage: true,
        },
            expectUnloadPage: true,
        },
        {
            content: "Verify blog created by targeting the rendered blog index",
            trigger: '#o_wblog_index_content',
            run: () => {},
        }
    ],
});
