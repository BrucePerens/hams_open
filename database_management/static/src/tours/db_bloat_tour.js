// This software is distributed under the terms of the Affero General Public License (AGPL-3).

/** @odoo-module **/
import { registry } from "@web/core/registry";

// Used elsewhere: o_tour_cancel_btn, o_tour_close_btn
registry.category("web_tour.tours").add("db_management_bloat_tour", { // # Verified by [@ANCHOR: COMM_test_db_bloat_tour]
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
            trigger: '.o_list_table input[type="checkbox"]',
            run: 'click',
        },
        {
            content: "Click Vacuum Analyze Selected",
            trigger: 'button[name="action_vacuum_analyze"]',
            run: 'click',
        },
        {
            content: "Wait for vacuum to finish",
            trigger: 'body',
            run: function () {},
        }
    ],
});
