/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("db_management_bloat_tour", { // # Verified by [@ANCHOR: test_db_bloat_tour]
    url: "/odoo?debug=1",
    steps: () => [
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
        {
            trigger: '.o_list_table',
            run: () => {},
        },
        {
            trigger: '.o_list_table .o_data_row .o_list_record_selector input',
            run: 'click',
        },
        {
            trigger: '*:contains("Vacuum Analyze Selected")',
            run: 'click',
        },
    ],
});
