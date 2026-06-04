/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("db_management_bloat_tour", { // # Verified by [@ANCHOR: test_db_bloat_tour]
    url: "/odoo?debug=1",
    steps: () => [
        {
            content: "Initialize Tour and Bypass Dialogs",
            trigger: 'body',
            run: function () {
                window.alert = function (msg) { console.warn("Alert: " + msg); };
                window.confirm = function (msg) { console.warn("Confirm: " + msg); return true; };
            },
        },
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
            content: "Wait for Table Health List View",
            trigger: '.o_list_renderer',
            run: function() {},
        },
        {
            content: "Select First Row",
            trigger: '.o_list_table .o_data_row .o_list_record_selector input',
            run: 'click',
        },
        {
            content: "Wait for Vacuum Analyze Button",
            trigger: 'button[name="action_vacuum_analyze"]:not([disabled])',
            run: function() {},
        },
        {
            content: "Click Vacuum Analyze Selected",
            trigger: 'button[name="action_vacuum_analyze"]',
            run: 'click',
        },
        {
            content: "Wait for RPC to complete (neutral wait)",
            trigger: 'body',
            run: function() {},
        }
    ],
});
