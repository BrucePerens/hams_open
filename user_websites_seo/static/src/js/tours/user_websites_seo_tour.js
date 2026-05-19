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
            trigger: '[name="user_websites_seo_settings"]',
            run: 'click',
        },
        {
            content: "Input SEO Meta Title",
            trigger: '.o_field_widget[name="website_meta_title"] input, input[id*="website_meta_title"]',
            run: 'edit SEO Title',
        },
        {
            content: "Input SEO Meta Description",
            trigger: '.o_field_widget[name="website_meta_description"] textarea, textarea[id*="website_meta_description"]',
            run: 'edit SEO Description',
        },
        {
            content: "Save the user record to ensure SEO fields are writeable",
            trigger: '.o_form_button_save, .o_form_button_check',
            run: 'click',
        },
        {
            content: "Wait for save to complete",
            trigger: '.o_form_saved, .o_form_button_create',
            run: () => {}, // wait
        },
        {
            content: "Go back to list to close the form",
            trigger: 'button[data-menu-xmlid="base.menu_users"]',
            run: 'click',
        }
    ],
});
