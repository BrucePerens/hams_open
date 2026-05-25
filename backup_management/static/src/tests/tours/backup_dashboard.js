/** @odoo-module **/

import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

registry.category("web_tour.tours").add("backup_dashboard_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        TourUtils.bypassDialogs(),
        { trigger: 'body', content: 'Initialize Tour' },
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
            trigger: '.o_select_menu_item',
            content: "Select Kopia engine value",
            run: function () {
                const items = document.querySelectorAll('.o_select_menu_item');
                let found = false;
                for (const item of items) {
                    if (item.textContent.includes('Kopia')) {
                        item.click();
                        found = true;
                        break;
                    }
                }
                if (!found) {
                    throw new Error("Kopia engine option not found in dropdown.");
                }
            }
        },
        {
            trigger: 'div[name="target_path"] input, input[id="target_path"]',
            run: "edit /var/lib/odoo/backups/tour_repo",
            content: "Enter target path",
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        }
    ].concat(TourUtils.safeSave('.o_form_button_save', '.o_form_saved, .o_form_button_create, .o_list_button_add')),
});

// # Tests [@ANCHOR: backup_dashboard_tour]
