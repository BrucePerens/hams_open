/** @odoo-module **/
/** Copyright © HAMS project. AGPL-3.0. **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_cf_purge_wizard]
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
            trigger: 'body:not(:has(.modal))',
            content: 'Wait for modal to disappear',
            run: function () {}
        },
        TourUtils.waitForAbsence('.modal', 'Purge Wizard Modal')
    ],
});
