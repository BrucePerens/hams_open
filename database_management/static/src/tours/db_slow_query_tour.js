/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("db_management_slow_query_tour", { // # Verified by [@ANCHOR: test_db_slow_query_tour]
    url: "/web",
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
            trigger: '[data-menu-xmlid="database_management.menu_db_queries"]',
            run: 'click',
        },
        {
            trigger: '.o_list_table',
            run: () => {},
        }
    ],
});
