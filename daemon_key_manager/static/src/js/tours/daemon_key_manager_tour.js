/** @odoo-module **/
// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: AGPL-3.0-or-later
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
            // The dropdown first lists the unfiltered results and re-renders when the
            // search for "facility" returns; clicking the first item races that
            // re-render under load and can leave the required field empty (save
            // then never settles). Wait (DOM-mutation driven, no timer) for the
            // filtered item, click it, then wait for the field to hold the value.
            trigger: 'div[name="user_id"] input',
            content: 'Click the filtered autocomplete item once it is rendered',
            run: function () {
                return new Promise((resolve) => {
                    const find = () => Array.from(
                        document.querySelectorAll('.o-autocomplete--dropdown-item')
                    ).find((el) => el.textContent.toLowerCase().includes('facility'));
                    const observer = new MutationObserver(() => {
                        const item = find();
                        if (item) {
                            observer.disconnect();
                            item.click();
                            resolve();
                        }
                    });
                    observer.observe(document.body, {childList: true, subtree: true, characterData: true});
                    const item = find();
                    if (item) {
                        observer.disconnect();
                        item.click();
                        resolve();
                    }
                });
            },
        },
        {
            trigger: 'div[name="user_id"] input:value("Odoo Facility Service Account")',
            content: 'Wait until the service account is actually set on the field',
        },
        {
            trigger: 'div[name="env_file_path"] input',
            content: 'Enter environment file path',
            run: 'edit /opt/hams/etc/keys/test.env',
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        }
    ].concat(TourUtils.safeSave()).concat([
        TourUtils.waitForElement('button[name="action_force_provision_all"]:not([disabled])', 'Force provision all keys button to be active'),
        {
            trigger: 'button[name="action_force_provision_all"]:not([disabled])',
            content: 'Force provision all keys',
            run: 'click',
        },
        TourUtils.waitForElement('.o_notification', 'success toast'),
        TourUtils.waitForAbsence('.o_notification', 'success toast to disappear'),
        {
            content: 'Click breadcrumb to return to list',
            trigger: '.o_control_panel .breadcrumb-item:not(.active):first, .o_control_panel .o_back_button',
            run: 'click',
        },
        {
            content: 'Wait for list view to load',
            trigger: '.o_list_button_add',
        },
        TourUtils.waitForAbsence('.o_form_view', 'form view to disappear'),
    ]),
});
