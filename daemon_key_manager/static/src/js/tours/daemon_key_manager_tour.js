/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("daemon_key_manager_tour", {
    url: "/odoo?debug=1&action=daemon_key_manager.action_daemon_key_registry",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_list_button_add',
            content: 'Create new registry entry',
            run: 'click',
        },
        {
            trigger: 'div[name="name"] input',
            content: 'Enter daemon name',
            run: 'edit TestDaemon',
        },
        {
            trigger: 'div[name="user_id"] input',
            content: 'Click to focus service account input',
            run: 'click',
        },
        {
            trigger: 'div[name="user_id"] input',
            content: 'Type service account name',
            run: 'edit facility',
        },
        {
            trigger: '.o-autocomplete--dropdown-item',
            content: 'Wait for autocomplete dropdown and click the item',
            run: 'click',
        },
        {
            trigger: 'div[name="env_file_path"] input',
            content: 'Enter environment file path',
            run: 'edit /var/lib/odoo/daemon_keys/test.env',
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        }
    ].concat(TourUtils.safeSave()).concat([
        {
            trigger: 'button[name="action_force_provision_all"]:not([disabled])',
            content: 'Force provision all keys (ensuring button is active)',
            run: 'click',
        },
        {
            content: 'Wait for success toast',
            trigger: '.o_notification_manager .o_notification',
        },
        TourUtils.waitForAbsence('.o_notification', 'success toast to disappear'),
        {
            content: 'Go back to list via breadcrumb to close the form',
            trigger: '.o_control_panel .breadcrumb-item a, .o_control_panel .o_back_button, .o_breadcrumb a',
            run: 'click',
        },
        {
            content: 'Wait for list view to load',
            trigger: '.o_list_button_add',
        }
    ]),
});
