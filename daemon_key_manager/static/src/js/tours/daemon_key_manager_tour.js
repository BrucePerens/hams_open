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
            run: (helpers) => TourUtils.deterministicInput(helpers, 'facility'),
        },
        {
            trigger: 'body',
            content: 'Wait for autocomplete dropdown and click the item',
            run: function () {
                return new Promise((resolve, reject) => {
                    let interval;
                    const timeoutId = setTimeout(() => {
                        clearInterval(interval);
                        reject(new Error("Service account 'facility' never appeared in dropdown."));
                    }, 10000);

                    interval = setInterval(() => {
                        const items = document.querySelectorAll('.dropdown-item, .o-autocomplete--dropdown-item');
                        for (const item of items) {
                            if (item.textContent.toLowerCase().includes('facility')) {
                                clearInterval(interval);
                                clearTimeout(timeoutId);
                                item.click();
                                resolve();
                                return;
                            }
                        }
                    }, 250);
                });
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
                    let interval;
                    const timeoutId = setTimeout(() => {
                        clearInterval(interval);
                        reject(new Error("Timeout waiting for rotation date or success toast."));
                    }, 18000);

                    interval = setInterval(() => {
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
