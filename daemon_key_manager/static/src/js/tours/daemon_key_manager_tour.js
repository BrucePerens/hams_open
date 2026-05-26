/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

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
            content: 'Input service account name manually and dispatch events',
            run: function () {
                const el = document.querySelector('div[name="user_id"] input');
                el.value = 'facility';
                el.dispatchEvent(new Event('input', { bubbles: true }));
                el.dispatchEvent(new Event('change', { bubbles: true }));
            }
        },
        {
            trigger: '.dropdown-item, .o-autocomplete--dropdown-item',
            content: 'Select the service account from OWL autocomplete',
            run: function () {
                const items = document.querySelectorAll('.dropdown-item, .o-autocomplete--dropdown-item');
                let found = false;
                for (const item of items) {
                    if (item.textContent.toLowerCase().includes('facility')) {
                        item.click();
                        found = true;
                        break;
                    }
                }
                if (!found) {
                    throw new Error("Service account 'facility' not found in dropdown.");
                }
            }
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
            content: 'Wait for the object button RPC to resolve and the rotation date to render',
            trigger: 'body',
            timeout: 20000,
            run: function () {
                return new Promise((resolve, reject) => {
                    const timeoutId = setTimeout(() => {
                        clearInterval(interval);
                        reject(new Error("Timeout waiting for rotation date or success toast."));
                    }, 18000);

                    const interval = setInterval(() => {
                        const field = document.querySelector('.o_field_widget[name="last_rotated"]');
                        const toast = document.querySelector('.toast-body') || document.querySelector('.o_notification_manager');
                        const errorDialog = document.querySelector('.modal-body.text-danger') || document.querySelector('.o_notification_manager .text-danger');

                        // Condition 1: Form reloaded successfully and field populated
                        if (field && /\d/.test(field.textContent)) {
                            clearInterval(interval);
                            clearTimeout(timeoutId);
                            resolve();
                        }
                        // Condition 2: Backend returned a notification instead of a form reload
                        else if (toast && toast.textContent.toLowerCase().includes('success')) {
                            clearInterval(interval);
                            clearTimeout(timeoutId);
                            resolve();
                        }
                        // Condition 3: Catch validation errors and fail fast
                        else if (errorDialog) {
                            clearInterval(interval);
                            clearTimeout(timeoutId);
                            reject(new Error("Backend returned an error: " + errorDialog.textContent));
                        }
                    }, 250);
                });
            }
        }
    ]),
});
