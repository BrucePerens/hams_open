/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// [@ANCHOR: test_tour_cf_waf_rule]
registry.category("web_tour.tours").add("cf_waf_rule_tour", {
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
            content: "Open WAF Rules Menu",
            trigger: '[data-menu-xmlid="cloudflare.menu_cf_waf_rules"]',
            run: "click"
        },
        {
            content: "Wait for WAF Rules list to render",
            trigger: 'th[data-name="action"]',
            run: function () {}
        },
        {
            trigger: 'tr.o_data_row',
            content: 'Check if Tour WAF rule exists in list',
            run: function () {
                const rows = document.querySelectorAll('tr.o_data_row');
                let found = false;
                for (const row of rows) {
                    if (row.textContent.includes('Tour XML-RPC Rule')) {
                        found = true;
                        break;
                    }
                }
                if (!found) {
                    throw new Error("Tour XML-RPC Rule not found");
                }
            }
        },
    ],
});
