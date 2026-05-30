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
            content: "Provide a valid downloadable URL pointing to the test controller",
            trigger: 'div[name="url"] input',
            run: function () {
                const input = document.querySelector('div[name="url"] input');
                input.value = document.location.origin + '/test/dummy_bin';
                input.dispatchEvent(new Event('input', { bubbles: true }));
                input.dispatchEvent(new Event('change', { bubbles: true }));
            },
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
            trigger: 'button[name="action_install"]',
            run: 'click',
        },
        {
            trigger: '.o_notification',
            content: "Wait for the success notification to ensure the RPC resolved",
            run: function () {
                const els = document.querySelectorAll('.o_notification');
                let found = false;
                for (const el of els) {
                    if (el.textContent.includes('Success')) {
                        found = true;
                        break;
                    }
                }
                if (!found) {
                    throw new Error('Success notification not found');
                }
            }
        },
    ]),
});
