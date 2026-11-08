/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Verified by [@ANCHOR: COMM_test_seo_widget_tour]
registry.category("web_tour.tours").add("user_websites_seo_tour", {
    url: "/odoo?debug=1&action=base.action_res_users",
    steps: () => [
        {
            // The action defaults to the "Internal Users" facet, which hides the portal test user.
            content: "Remove the default Internal Users facet",
            trigger: '.o_searchview .o_facet_remove',
            run: 'click',
        },
        {
            // Search for the test user rather than looking for the row on the first page: the users
            // list shows 80, and on a database with more users than that the new one can sort past them.
            content: "Search for the test user by name",
            trigger: '.o_searchview_input',
            run: 'edit SEO UI Test User',
        },
        {
            content: "Run the search",
            trigger: '.o_searchview_input',
            run: 'press Enter',
        },
        {
            content: "Open the test user's row",
            trigger: '.o_data_row:has(.o_data_cell[name="name"]:contains("SEO UI Test User")) .o_data_cell[name="name"]',
            run: 'click',
        },
        {
            content: "Click the SEO Metadata notebook tab injected by our module",
            trigger: 'a[name="user_websites_seo_settings"]',
            run: 'click',
        },
        {
            content: "Input SEO Meta Title",
            trigger: '.o_field_widget[name="website_meta_title"] input',
            run: 'edit SEO Title',
        },
        {
            content: "Input SEO Meta Description",
            trigger: '.o_field_widget[name="website_meta_description"] textarea',
            run: 'edit SEO Description',
        },
        {
            content: "Input SEO Meta Keywords",
            trigger: '.o_field_widget[name="website_meta_keywords"] input',
            run: 'edit SEO, Keywords, Odoo',
        },
        {
            content: 'Click away to force DOM blur and commit text input',
            trigger: '.o_form_sheet',
            run: 'click',
        }
    ].concat(TourUtils.safeSave()).concat([
        {
            content: 'Go back to list via breadcrumb to close the form',
            trigger: '.o_control_panel .breadcrumb-item:not(.active):first, .o_control_panel .o_back_button',
            run: 'click',
        },
        {
            content: "Wait for list view to load",
            trigger: '.o_list_table',
            run: function() {}
        },
    ]),
});
