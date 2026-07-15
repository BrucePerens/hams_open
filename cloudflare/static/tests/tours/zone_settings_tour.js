/** @odoo-module **/
/** Copyright © HAMS project. AGPL-3.0. **/
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
            trigger: '.o_breadcrumb',
            content: "Wait for App Breadcrumb to render",
            run: function () {}
        },
        {
            content: "Open Zone Settings Menu",
            trigger: 'a[data-menu-xmlid="cloudflare.menu_cf_zone_settings"]:not(:visible), a[data-menu-xmlid="cloudflare.menu_cf_zone_settings"]',
            run: function (actions) {
                const link = document.querySelector('a[data-menu-xmlid="cloudflare.menu_cf_zone_settings"]');
                // Check if hidden (offsetHeight is 0)
                if (link && link.offsetHeight === 0) {
                    const moreBtn = document.querySelector('.o_menu_sections_more.dropdown-toggle');
                    if (moreBtn) {
                        moreBtn.click();
                    }
                }
                // setTimeout(() => {
                    document.querySelector('a[data-menu-xmlid="cloudflare.menu_cf_zone_settings"]').click();
            }
        },
        { trigger: '.modal-content', content: 'Wait for Zone Settings Modal to mount and render', run: function() {} },
        {
            content: "Click Apply Settings button",
            trigger: 'button[name="action_apply_settings"]',
            run: "click"
        },
        TourUtils.waitForAbsence('.modal', 'Zone Settings Modal')
    ],
});
