/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("cf_purge_wizard_tour", {
    url: "/odoo",
    steps: () => [
        {
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            content: "Open Cloudflare Edge",
            trigger: '[data-menu-xmlid="cloudflare.menu_cloudflare_root"]',
            run: "click"
        },
        {
            content: "Open Purge Cache Wizard",
            trigger: 'a[data-menu-xmlid="cloudflare.menu_cf_purge_wizard"]',
            run: "click"
        },
        {
            content: "Click Purge button (Everything)",
            trigger: 'button[name="action_purge"]',
            run: "click"
        },
        {
            content: "Verify notification",
            trigger: '.o_notification_manager .o_notification:contains("successfully")',
        }
    ],
});
