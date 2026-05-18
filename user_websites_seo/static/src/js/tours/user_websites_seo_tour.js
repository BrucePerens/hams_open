/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// Verified by [@ANCHOR: test_seo_widget_tour]
registry.category("web_tour.tours").add("user_websites_seo_tour", {
    url: "/odoo",
    steps: () => [
        {
            trigger: '.o_navbar_apps_menu button',
            content: "Open Apps Menu",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="base.menu_administration"]',
            content: "Open Settings",
            run: "click",
        },
        {
            trigger: 'button[data-menu-xmlid="base.menu_users"]',
            content: "Open Users & Companies menu",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="base.menu_action_res_users"]',
            content: "Open Users",
            run: "click",
        },
        {
            trigger: '.o_facet_remove',
            content: "Remove default Internal Users filter",
            run: "click",
        },
        {
            trigger: 'td.o_data_cell:contains("SEO UI Test User")',
            content: "Open User Form Directly (Bypass fragile search logic)",
            run: "click",
        },
        {
            content: "Click the SEO Metadata notebook tab injected by our module",
            trigger: '*:contains("SEO Metadata")',
            run: 'click',
        },
        {
            content: "Input SEO Meta Title",
            trigger: 'div[name="website_meta_title"] input',
            run: 'edit SEO Title',
        },
        {
            content: "Input SEO Meta Description",
            trigger: 'div[name="website_meta_description"] textarea',
            run: 'edit SEO Description',
        },
        {
            content: "Save the user record to ensure SEO fields are writeable",
            trigger: '.o_form_button_save',
            run: 'click',
        },
        {
            content: "Wait for save to complete",
            trigger: '.o_form_saved',
        }
    ],
});
