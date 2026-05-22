/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// [@ANCHOR: test_tour_cf_ip_ban]
registry.category("web_tour.tours").add("cf_ip_ban_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        TourUtils.clickElement('[data-menu-xmlid="cloudflare.menu_cloudflare_root"], *:contains("Cloudflare Edge")', "Open App Switcher (if needed) or click Cloudflare Edge"),
        {
            content: "Open IP Bans Menu",
            trigger: 'a[data-menu-xmlid="cloudflare.menu_cf_ip_bans"]',
            run: "click"
        },
        TourUtils.waitForElement('tr.o_data_row td:contains("192.168.9.9")', 'Check if ban record exists in the list view'),
        TourUtils.clickElement('tr.o_data_row td:contains("192.168.9.9")', "Click on the IP Ban record to open form view"),
        TourUtils.waitForElement('button[name="action_lift_ban"]', 'Verify Lift Ban button is rendered'),
        TourUtils.waitForRPC()
    ],
});
