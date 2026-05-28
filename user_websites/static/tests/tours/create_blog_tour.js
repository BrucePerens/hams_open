/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_create_blog]
// Tests [@ANCHOR: controller_user_blog_index]
// Tests [@ANCHOR: UX_CREATE_BLOG_POST]
registry.category("web_tour.tours").add("create_blog_tour", {
    url: "/blogtour/blog",
    steps: () => [
        { trigger: 'button.o_tour_create_site_btn', content: 'Wait for: Wait for Create Blog Button', run: function() {} },
        {
            content: "Click Create using namespaced fallback class",
            trigger: 'button.o_tour_create_site_btn',
            run: 'click',
            expectUnloadPage: true,
        },
        {
            content: "Verify blog created by targeting the community namespaced dropzone",
            trigger: '#user_websites_dropzone_blog_header:not(:visible)',
            run: () => {},
        }
    ],
});
