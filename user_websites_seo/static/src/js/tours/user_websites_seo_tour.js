/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// Verified by [@ANCHOR: test_seo_widget_tour]
registry.category("web_tour.tours").add("user_websites_seo_tour", {
    url: "/odoo",
    steps: () => [
        TourUtils.waitForElement('.o_navbar_apps_menu button', 'Wait for Apps Menu Button'),
        {
            trigger: '.o_navbar_apps_menu button',
            content: "Open Apps Menu",
            run: "click",
        },
        TourUtils.waitForElement('[data-menu-xmlid="base.menu_administration"]', 'Wait for Settings Menu'),
        {
            trigger: '[data-menu-xmlid="base.menu_administration"]',
            content: "Open Settings",
            run: "click",
        },
        TourUtils.waitForElement('button[data-menu-xmlid="base.menu_users"]', 'Wait for Users Top Menu'),
        {
            trigger: 'button[data-menu-xmlid="base.menu_users"]',
            content: "Open Users & Companies menu",
            run: "click",
        },
        TourUtils.waitForElement('[data-menu-xmlid="base.menu_action_res_users"]', 'Wait for Users Submenu'),
        {
            trigger: '[data-menu-xmlid="base.menu_action_res_users"]',
            content: "Open Users",
            run: "click",
        },
        TourUtils.waitForElement('.o_facet_remove', 'Wait for default filter to appear'),
        {
            trigger: '.o_facet_remove',
            content: "Remove default Internal Users filter",
            run: "click",
        },
        TourUtils.waitForElement('td.o_data_cell:contains("SEO UI Test User")', 'Wait for user row to render'),
        {
            trigger: 'td.o_data_cell:contains("SEO UI Test User")',
            content: "Open User Form Directly (Bypass fragile search logic)",
            run: "click",
        },
        TourUtils.waitForElement('.o_form_sheet a[name="user_websites_seo_settings"]', 'Wait for form sheet and SEO tab to hydrate'),
        {
            content: "Click the SEO Metadata notebook tab injected by our module",
            trigger: 'a[name="user_websites_seo_settings"]',
            run: 'click',
        },
        TourUtils.waitForElement('.o_field_widget[name="website_meta_title"] input, input[id*="website_meta_title"]', 'Wait for title input'),
        {
            content: "Input SEO Meta Title",
            trigger: '.o_field_widget[name="website_meta_title"] input, input[id*="website_meta_title"]',
            run: 'edit SEO Title',
        },
        TourUtils.waitForElement('.o_field_widget[name="website_meta_description"] textarea, textarea[id*="website_meta_description"]', 'Wait for description input'),
        {
            content: "Input SEO Meta Description",
            trigger: '.o_field_widget[name="website_meta_description"] textarea, textarea[id*="website_meta_description"]',
            run: 'edit SEO Description',
        },
        ...TourUtils.safeSave('.o_form_button_save, .o_form_button_check', '.o_form_saved, .o_form_button_create'),
        {
            content: "Go back to list to close the form",
            trigger: 'button[data-menu-xmlid="base.menu_users"]',
            run: 'click',
        }
    ],
});
