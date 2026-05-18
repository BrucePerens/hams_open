/** @odoo-module **/
import { registry } from "@web/core/registry";

// [@ANCHOR: test_tour_cf_ip_ban]
registry.category("web_tour.tours").add("cf_ip_ban_tour", {
    url: "/odoo",
    steps: () => [
        {
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            content: "Open App Switcher (if needed) or click Cloudflare Edge",
            trigger: '[data-menu-xmlid="cloudflare.menu_cloudflare_root"], *:contains("Cloudflare Edge")',
            run: "click"
        },
        {
            content: "Open IP Bans Menu",
            trigger: 'a[data-menu-xmlid="cloudflare.menu_cf_ip_bans"]',
            run: "click"
        },
        {
            content: "Check if ban record exists in the list view",
            trigger: 'tr.o_data_row td:contains("192.168.9.9")',
            run: () => {
                if (!document.querySelector('tr.o_data_row td')) {
                    console.error("Ban record missing from DOM");
                }
            }
        },
        {
            content: "Click on the IP Ban record to open form view",
            trigger: 'tr.o_data_row td:contains("192.168.9.9")',
            run: "click"
        },
        {
            content: "Verify Lift Ban button is rendered",
            trigger: 'button[name="action_lift_ban"]',
            run: () => {
                if (!document.querySelector('button[name="action_lift_ban"]')) {
                    console.error("Lift Ban button missing from DOM");
                }
            }
        }
    ],
});
