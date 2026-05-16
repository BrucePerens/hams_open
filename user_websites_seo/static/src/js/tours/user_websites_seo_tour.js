/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// Verified by [@ANCHOR: test_seo_widget_tour]
registry.category("web_tour.tours").add("user_websites_seo_tour", {
    steps: () => [
        {
            content: "Wait for form to load",
            trigger: '.o_form_sheet',
            run: () => {},
        },
        {
            content: "Click the SEO Metadata notebook tab injected by our module",
            trigger: 'a[name="user_websites_seo_settings"]',
            run: 'click',
        },
        {
            content: "Edit the SEO Meta Title",
            trigger: 'div[name="website_meta_title"] input',
            run: 'edit Test SEO Title',
        },
        {
            content: "Edit the SEO Meta Description",
            trigger: 'div[name="website_meta_description"] input',
            run: 'edit Test SEO Description',
        },
        ...TourUtils.safeSave(),
        {
            content: "Finish Tour",
            trigger: 'body',
            run: () => {},
        }
    ],
});
