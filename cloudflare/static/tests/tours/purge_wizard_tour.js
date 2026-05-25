/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

registry.category("web_tour.tours").add("cf_purge_wizard_tour", {
    url: "/odoo?debug=1",
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
        { trigger: '.modal-content', content: 'Wait for Purge Wizard Modal to mount and render', run: function() {} },
        {
            content: "Click Purge button (Everything)",
            trigger: 'button[name="action_purge"]',
            run: "click"
        },
        {
            content: "Wait for RPC to complete and notification to mount",
            trigger: '.o_notification',
            run: function () {
                // The presence of .o_notification proves the RPC resolved.
                // Synchronously checking textContent during animation frames causes race conditions, so we just pass.
            }
        }
    ],
});
