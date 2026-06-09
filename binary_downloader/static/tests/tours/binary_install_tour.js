/** @odoo-module **/
// # Verified by [@ANCHOR: test_binary_install_tour]
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("binary_install_tour", {
    url: "/odoo?debug=1&action=binary_downloader.action_binary_downloader_manifest",
    steps: () => [
        TourUtils.waitForAbsence('.o_loading', 'Wait for initial load'),
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_list_button_add',
            run: 'click',
        },
        {
            trigger: 'div[name="name"] input',
            run: 'edit tourbin',
        },
        {
            trigger: 'div[name="url"] input',
            run: function (helpers) {
                // Ensure focus for deterministicInput
                document.querySelector('div[name="url"] input').focus();
            }
        },
        {
            content: "Provide a valid downloadable URL pointing to the test controller",
            trigger: 'div[name="url"] input',
            run: function (helpers) {
                TourUtils.deterministicInput(helpers, document.location.origin + '/test/dummy_bin');
            },
        },
        {
            trigger: 'div[name="checksum"] input',
            run: function (helpers) {
                document.querySelector('div[name="checksum"] input').focus();
            }
        },
        {
            content: "Provide the exact SHA256 hash for the string '1234'",
            trigger: 'div[name="checksum"] input',
            run: 'edit 03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4',
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        }
    ].concat(TourUtils.safeSave()).concat([
        {
            content: "Click Install Now using immutable name attribute",
            trigger: 'button[name="action_install"]:not(:disabled)',
            run: 'click',
        },
        {
            trigger: '.o_notification_manager .o_notification',
            content: "Wait for the success notification to ensure the RPC resolved",
            run: function () {
                const notifications = document.querySelectorAll('.o_notification');
                for (const note of notifications) {
                    if (note.innerText.includes('Success')) {
                        return;
                    }
                }
                throw new Error('Success notification not found');
            }
        },
    ]),
});
