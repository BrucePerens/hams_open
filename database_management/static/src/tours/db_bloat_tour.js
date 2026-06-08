/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("db_management_bloat_tour", { // # Verified by [@ANCHOR: test_db_bloat_tour]
    url: "/odoo?debug=1",
    steps: () => [
        {
            content: "Open Apps Menu",
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            content: "Select Database & SRE App",
            trigger: '[data-menu-xmlid="database_management.menu_admin_root"]',
            run: 'click',
        },
        {
            trigger: '.o_list_renderer',
            run: function () {},
        },
        {
            content: "Select First Row",
            trigger: '.o_list_table .o_data_row .o_list_record_selector input',
            run: 'click',
        },
        {
            content: "Click Vacuum Analyze Selected",
            trigger: 'button[name="action_vacuum_analyze"]',
            run: 'click',
        },
    ],
});
