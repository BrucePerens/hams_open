/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_cf_zone_settings]
registry.category("web_tour.tours").add("cf_zone_settings_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            content: "Open Apps Menu",
            trigger: '.o_navbar_apps_menu button',
            run: "click"
        },
        { trigger: '[data-menu-xmlid="cloudflare.menu_cloudflare_root"]', content: "Open Cloudflare Edge Menu", run: 'click' },
        {
            content: "Open Zone Settings Menu",
            trigger: '.o_main_navbar a[data-menu-xmlid="cloudflare.menu_cf_zone_settings"], header.o_navbar a[data-menu-xmlid="cloudflare.menu_cf_zone_settings"]',
            run: "click"
        },
        { trigger: '.modal-content', content: 'Wait for Zone Settings Modal to mount and render', run: function() {} },
        {
            content: "Click Apply Settings button",
            trigger: 'button[name="action_apply_settings"]',
            run: "click"
        },
        { trigger: 'body', run: function() {} } // Final step to wait for click resolution
    ],
});
