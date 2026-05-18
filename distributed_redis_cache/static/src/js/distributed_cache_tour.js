/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("distributed_cache_admin_tour", {
    url: "/odoo",
    steps: () => [
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
        {
            trigger: '.o_field_widget[name="model_id"] input',
            content: "Select a model",
            run: "edit User",
        },
        {
            trigger: '.dropdown-item:contains("User"), .ui-menu-item *:contains("User")',
            run: "click",
        },
        {
            trigger: 'button[name="action_invalidate_model_cache"]',
            content: "Invalidate the cache",
            run: "click",
        },
        {
            trigger: '.toast-body:contains("Success"), .o_notification_manager *:contains("Success")',
            content: "Verify success message",
        }
    ]
});
