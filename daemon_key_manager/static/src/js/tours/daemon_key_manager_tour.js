/** @odoo-module **/

import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// # Verified by [@ANCHOR: test_daemon_key_manager_tour]
registry.category("web_tour.tours").add("daemon_key_manager_tour", {
    url: "/odoo?debug=1&action=daemon_key_manager.action_daemon_key_registry",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_list_button_add',
            content: "Create new registry entry",
            run: "click",
        },
        {
            trigger: 'div[name="name"] input',
            content: "Enter daemon name",
            run: "edit UI Tour Daemon",
        },
        {
            trigger: 'div[name="user_id"] input',
            content: "Click to focus service account input",
            run: "click",
        },
        {
            trigger: 'div[name="user_id"] input',
            content: "Type service account name",
            run: "edit Daemon Key",
        },
        TourUtils.clickElement('.o-autocomplete--dropdown-item:contains("Daemon Key Manager Service"), .dropdown-item:contains("Daemon Key Manager Service")', "Select the service account from OWL autocomplete"),
        {
            trigger: 'div[name="env_file_path"] input',
            content: "Enter environment file path",
            run: "edit /var/lib/odoo/daemon_keys/tour.env",
        }
    ].concat(TourUtils.safeSave()).concat([
        {
            trigger: 'button[name="action_force_provision_all"]',
            content: "Force provision all keys",
            run: "click",
        },
        TourUtils.waitForRPC()
    ]),
});
