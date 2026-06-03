/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("distributed_cache_admin_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_main_navbar',
            content: "Wait for navbar",
        },
        {
            trigger: '.o_navbar_apps_menu button',
            content: "Open Apps Menu",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="distributed_redis_cache.menu_distributed_cache_root"]',
            content: "Open Distributed Cache Manager",
            run: "click",
        },
        {
            trigger: 'button[name="check_redis_status"]',
            content: "Check Redis Status",
            run: "click",
        },
        {
            trigger: '.o_notification',
            content: "Wait for status notification to appear",
            run: function() {}
        },
        {
            trigger: '.o_field_widget[name="model_id"] input',
            content: "Input model name using native simulator",
            run: "edit User",
        },
        {
            trigger: '.o-autocomplete--dropdown-item',
            content: "Wait for autocomplete dropdown and click the item",
            run: function () {
                const items = document.querySelectorAll('.o-autocomplete--dropdown-item, .dropdown-item');
                for (const item of items) {
                    if (item.textContent.includes('User')) {
                        item.click();
                        return;
                    }
                }
            }
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        },
        {
            trigger: 'button[name="action_invalidate_model_cache"]',
            content: "Invalidate the cache",
            run: "click",
        },
        TourUtils.waitForAbsence('.o_notification', 'Wait for notification to disappear'),
    ]
});
