/** @odoo-module **/
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
            trigger: 'tr.o_data_row td',
            content: "Wait for and click on the IP Ban record to open form view",
            run: function () {
                const items = document.querySelectorAll('tr.o_data_row td');
                for (const item of items) {
                    if (item.textContent.includes('192.168.9.9')) {
                        item.click();
                        break;
                    }
                }
            }
        },
        { trigger: 'button[name="action_lift_ban"]', content: 'Wait for: Verify Lift Ban button is rendered', run: function() {} },

    ],
});
