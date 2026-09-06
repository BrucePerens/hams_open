/** @odoo-module **/
// SPDX-License-Identifier: AGPL-3.0-or-later
// Tests [@ANCHOR: COMM_distributed_cache_view]
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("distributed_cache_admin_tour", {
    // menu_distributed_cache_root is parented under base.menu_custom, which
    // does not render as a tile in the .o_navbar_apps_menu grid that button
    // opens (that grid is application roots only) -- clicking through it and
    // waiting for this menu's own data-menu-xmlid never found the element,
    // confirmed via two real, consistent test-suite failures, not a one-off
    // flake. Every other tour in this codebase already deep-links straight
    // into its own action instead of navigating the Apps menu by hand; doing
    // the same here sidesteps the whole question of where base.menu_custom
    // items render.
    url: "/odoo?debug=1&action=distributed_redis_cache.action_distributed_cache_config",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_main_navbar',
            content: "Wait for navbar",
        },
        {
            trigger: 'button[name="check_redis_status"]',
            content: "Check Redis Status",
            run: "click",
        },
        TourUtils.waitForElement('.o_notification', "Wait for status notification to appear"),
        {
            trigger: '.o_field_widget[name="model_id"] input',
            content: "Input model name using native simulator",
            run: "edit User",
        },
        {
            trigger: '.o-autocomplete--dropdown-item',
            content: "Wait for autocomplete dropdown and click the item",
            run: 'click',
        },
        {
            trigger: '.o_form_view',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        },
        {
            trigger: 'button[name="action_invalidate_model_cache"]',
            content: "Invalidate the cache",
            run: "click",
        },
        TourUtils.waitForElement('.o_notification', "Wait for notification to appear"),
        TourUtils.waitForAbsence('.o_notification', 'Wait for notification to disappear'),
    ]
});
