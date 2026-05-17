/** @odoo-module **/

import { registry } from "@web/core/registry";

// # Verified by [@ANCHOR: test_daemon_key_manager_tour]
registry.category("web_tour.tours").add("daemon_key_manager_tour", {
    url: "/odoo?action=daemon_key_manager.action_daemon_key_registry",
    steps: () => [
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
            trigger: '.o_form_button_create',
            content: "Wait for save to complete by observing the New button",
            run: () => {},
        },
        {
            trigger: 'button[name="action_force_provision_all"]',
            content: "Force provision all keys",
            run: "click",
        }
    ],
});
