/** @odoo-module **/

import { registry } from "@web/core/registry";

// # Verified by [@ANCHOR: test_daemon_key_manager_tour]
registry.category("web_tour.tours").add("daemon_key_manager_tour", {
    url: "/odoo",
    steps: () => [
        {
            trigger: '.o_list_renderer, .o_list_button_add',
            content: "Wait for list view",
            run: () => {},
        },
        {
            trigger: 'button.o_list_button_add',
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
            content: "Select service account",
            run: "edit daemon_key_manager_service",
        },
        {
            trigger: '.ui-menu-item *:contains("Daemon Key Manager Service")',
            content: "Select the service account from autocomplete",
            run: "click",
        },
        {
            trigger: 'div[name="env_file_path"] input',
            content: "Enter environment file path",
            run: "edit /var/lib/odoo/daemon_keys/tour.env",
        },
        {
            trigger: 'button.o_form_button_save',
            content: "Save the registry entry",
            run: "click",
        },
        {
            trigger: 'button[name="action_force_provision_all"]',
            content: "Force provision all keys",
            run: "click",
        },
        {
            trigger: '.o_list_renderer',
            content: "Wait for return to list view (assuming the action returns to list or stays on form)",
            run: () => {}, // Just a check
        },
    ],
});
