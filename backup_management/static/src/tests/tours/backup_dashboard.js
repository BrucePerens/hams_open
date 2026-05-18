/** @odoo-module **/

import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("backup_dashboard_tour", {
    url: "/odoo",
    steps: () => [
        {
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="backup_management.menu_admin_root"]',
            content: "Click on Backup Management app",
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="backup_management.menu_backup_root"]',
            content: "Click on Backups Submenu",
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="backup_management.menu_backup_config"]',
            content: "Open Configurations",
            run: 'click',
        },
        {
            trigger: ".o_list_button_add",
            content: "Create new configuration",
            run: 'click',
        },
        {
            trigger: 'div[name="name"] input, input[id="name"]',
            run: "edit Test Kopia Tour",
            content: "Enter name",
        },
        {
            trigger: 'div[name="engine"] .o_select_menu_toggler',
            content: "Open engine dropdown",
            run: 'click',
        },
        {
            trigger: '.o_select_menu_item *:contains("Kopia")',
            content: "Select Kopia engine value",
            run: 'click',
        },
        {
            trigger: 'div[name="target_path"] input, input[id="target_path"]',
            run: "edit /var/lib/odoo/backups/tour_repo",
            content: "Enter target path",
        },
        {
            trigger: ".o_form_button_save",
            content: "Save configuration",
            run: "click",
        },
        {
            trigger: ".o_breadcrumb, .o_form_button_create",
            content: "Verify saved",
            run: () => {},
        }
    ],
});

// # Verified by [@ANCHOR: test_tour_execution]
