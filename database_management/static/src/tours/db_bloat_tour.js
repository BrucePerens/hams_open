/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("db_management_bloat_tour", { // # Verified by [@ANCHOR: test_db_bloat_tour]
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="database_management.menu_admin_root"]',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="database_management.menu_db_root"]',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="database_management.menu_db_tables"]',
            run: 'click',
        },
        { trigger: '.o_list_table', content: 'Wait for: Wait for table to render', run: function() {} },
        {
            trigger: '.o_control_panel:has(.o_breadcrumb)',
            content: "Wait for breadcrumbs to ensure page loaded",
            run: function() {},
        },
        {
            trigger: '.o_list_table .o_data_row .o_list_record_selector input',
            run: 'click',
        },
        {
            trigger: 'button[name="action_vacuum_analyze"]:not([disabled])',
            content: "Wait for button to be enabled",
            run: function() {},
        },
        {
            trigger: 'button[name="action_vacuum_analyze"]',
            content: "Vacuum Analyze Selected",
            run: 'click',
        },
        {
            trigger: 'body',
            run: function() {},
        }
    ],
});
