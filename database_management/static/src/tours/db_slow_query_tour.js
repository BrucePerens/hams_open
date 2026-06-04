/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("db_management_slow_query_tour", { // # Verified by [@ANCHOR: test_db_slow_query_tour]
    url: "/odoo?debug=1",
    steps: () => [
        {
            content: "Wait for navbar",
            trigger: '.o_navbar',
            run: function() {},
        },
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
            content: "Open Database Health Menu",
            trigger: '[data-menu-xmlid="database_management.menu_db_root"]',
            run: 'click',
        },
        {
            content: "Go to Slow Queries",
            trigger: '[data-menu-xmlid="database_management.menu_db_queries"]',
            run: 'click',
        },
        {
            content: "Wait for Slow Query List View",
            trigger: '.o_list_renderer',
            run: function() {},
        },
    ],
});
