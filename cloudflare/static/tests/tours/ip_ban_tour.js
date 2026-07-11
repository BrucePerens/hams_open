/** @odoo-module **/
/** Copyright © HAMS project. AGPL-3.0. **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_cf_ip_ban]
registry.category("web_tour.tours").add("cf_ip_ban_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            content: 'Open App Switcher Dropdown',
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            content: "Click Cloudflare Edge App",
            trigger: '[data-menu-xmlid="cloudflare.menu_cloudflare_root"]',
            run: 'click',
        },
        {
            trigger: '.o_breadcrumb',
            content: "Wait for App Breadcrumb to render",
            run: function () {}
        },
        {
            content: "Open IP Bans Menu",
            trigger: 'a[data-menu-xmlid="cloudflare.menu_cf_ip_bans"]',
            run: "click"
        },
        {
            content: "Wait for IP Bans list to render",
            trigger: '.o_list_renderer',
            run: function () {}
        },
        {
            trigger: 'tr.o_data_row td[name="ip_address"]',
            content: "Wait for data row and click to open form view",
            run: 'click',
        },
        {
            trigger: '.o_form_sheet',
            content: "Wait for form view to render",
            run: function () {}
        },
        { trigger: 'button[name="action_lift_ban"]', content: 'Wait for: Verify Lift Ban button is rendered', run: function() {} },

    ],
});
