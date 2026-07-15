/** @odoo-module **/
import { registry } from "@web/core/registry";


// [@ANCHOR: test_tour_create_site]

// Tests [@ANCHOR: controller_user_websites_home]

// Tests [@ANCHOR: UX_CREATE_SITE]
registry.category("web_tour.tours").add("create_site_tour", {
    url: "/sitetour/home",
    steps: () => [
        { trigger: 'button.o_tour_create_site_btn', content: 'Wait for: Wait for Create Site Button', run: function() {} },
        {
            content: "Click Create using namespaced fallback class",
            trigger: 'button.o_tour_create_site_btn',
            run: 'click',
            expectUnloadPage: true,
        },
        { trigger: 'body', content: 'Wait for page reload to hydrate DOM', run: function() {} },
        {
            content: "Verify site created (targeting invisible dropzone with native pseudo-selector)",
            trigger: '#user_websites_dropzone_home_header:not(:visible)',
            run: () => {},
        }
    ],
});
