/** @odoo-module **/

import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

registry.category("web_tour.tours").add("zero_sudo_tour", {
    // [@ANCHOR: zero_sudo_tour]
    // Verified by [@ANCHOR: test_zero_sudo_tour]
    // Tests [@ANCHOR: story_login_blocking]
    // Tests [@ANCHOR: journey_service_account_lifecycle]
    url: "/odoo",
    steps: () => [
        {
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="base.menu_administration"]',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="base.menu_users"]',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="base.menu_action_res_users"]',
            run: 'click',
        },
        {
            trigger: '.o_list_button_add',
            content: "Create a new user",
            run: 'click',
        },
        {
            trigger: 'div[name="name"] input',
            run: 'edit Tour Service Account',
        },
        {
            trigger: 'div[name="login"] input',
            run: 'edit tour_service_account',
        },
        {
            trigger: 'div[name="is_service_account"] input',
            run: 'click',
        },
        ...TourUtils.safeSave()
    ],
});
