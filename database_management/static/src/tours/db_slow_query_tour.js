// This software is distributed under the terms of the Affero General Public License (AGPL-3).

/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("db_management_slow_query_tour", { // # Verified by [@ANCHOR: COMM_test_db_slow_query_tour]
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
            trigger: '.o_list_renderer',
            run: function () {},
        },
    ],
});
