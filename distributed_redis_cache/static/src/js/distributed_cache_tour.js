/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

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
            trigger: '.o_notification_manager',
            content: "Wait for status notification",
        },
        TourUtils.deterministicInput('.o_field_widget[name="model_id"] input', 'User'),
        TourUtils.clickElement('.dropdown-item:contains("User"), .o-autocomplete--dropdown-item:contains("User")', "Select the model from autocomplete"),
        {
            trigger: 'button[name="action_invalidate_model_cache"]',
            content: "Invalidate the cache",
            run: "click",
        },
        TourUtils.waitForElement('.toast-body:contains("Success"), .o_notification_manager *:contains("Success")', "Verify success message"),
        TourUtils.waitForRPC()
    ]
});
