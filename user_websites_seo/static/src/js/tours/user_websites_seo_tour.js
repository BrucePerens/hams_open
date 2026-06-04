/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Verified by [@ANCHOR: test_seo_widget_tour]
registry.category("web_tour.tours").add("user_websites_seo_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        { trigger: '.o_navbar_apps_menu button', content: 'Wait for: Wait for Apps Menu Button', run: function() {} },
        {
            trigger: '.o_navbar_apps_menu button',
            content: "Open Apps Menu",
            run: "click",
        },
        { trigger: '[data-menu-xmlid="base.menu_administration"]', content: 'Wait for: Wait for Settings Menu', run: function() {} },
        {
            trigger: '[data-menu-xmlid="base.menu_administration"]',
            content: "Open Settings",
            run: "click",
        },
        { trigger: 'button[data-menu-xmlid="base.menu_users"]', content: 'Wait for: Wait for Users Top Menu', run: function() {} },
        {
            trigger: 'button[data-menu-xmlid="base.menu_users"]',
            content: "Open Users & Companies menu",
            run: "click",
        },
        { trigger: '[data-menu-xmlid="base.menu_action_res_users"]', content: 'Wait for: Wait for Users Submenu', run: function() {} },
        {
            trigger: '[data-menu-xmlid="base.menu_action_res_users"]',
            content: "Open Users",
            run: "click",
        },
        { trigger: '.o_facet_remove', content: 'Wait for: Wait for default filter to appear', run: function() {} },
        {
            trigger: '.o_facet_remove',
            content: "Remove default Internal Users filter",
            run: "click",
        },
        {
            content: "Click the specific user row",
            trigger: '.o_data_row .o_data_cell[name="name"]',
            run: 'click',
        },
        { trigger: '.o_form_sheet a[name="user_websites_seo_settings"]', content: 'Wait for: Wait for form sheet and SEO tab to hydrate', run: function() {} },
        {
            content: "Click the SEO Metadata notebook tab injected by our module",
            trigger: 'a[name="user_websites_seo_settings"]',
            run: 'click',
        },
        { trigger: '.o_field_widget[name="website_meta_title"] input, input[id*="website_meta_title"]', content: 'Wait for: Wait for title input', run: function() {} },
        {
            content: "Input SEO Meta Title",
            trigger: '.o_field_widget[name="website_meta_title"] input, input[id*="website_meta_title"]',
            run: 'edit SEO Title',
        },
        { trigger: '.o_field_widget[name="website_meta_description"] textarea, textarea[id*="website_meta_description"]', content: 'Wait for: Wait for description input', run: function() {} },
        {
            content: "Input SEO Meta Description",
            trigger: '.o_field_widget[name="website_meta_description"] textarea, textarea[id*="website_meta_description"]',
            run: 'edit SEO Description',
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        }
    ].concat(TourUtils.safeSave('.o_form_button_save, .o_form_button_check', '.o_form_saved, .o_form_button_create')).concat([
        {
            content: "Go back to list via breadcrumb to close the form",
            trigger: '.o_control_panel .breadcrumb-item a, .o_control_panel .o_back_button, .o_breadcrumb a',
            run: 'click',
        },
        {
            content: "Wait for list view to load",
            trigger: '.o_list_table',
            run: function() {}
        },
    ]),
});
