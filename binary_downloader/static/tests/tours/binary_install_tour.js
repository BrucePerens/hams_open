/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("binary_install_tour", {
    url: "/web",
    steps: () => [
        {
            trigger: '.o_navbar_apps_menu button',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="base.menu_administration"]',
            run: 'click',
        },
        {
            trigger: '.o_settings_container, [data-menu-xmlid="base.menu_custom"], *:contains("Custom")',
            run: () => {},
        },
        {
            trigger: '[data-menu-xmlid="base.menu_custom"], *:contains("Custom")',
            run: 'click',
        },
        {
            trigger: '[data-menu-xmlid="binary_downloader.menu_binary_downloader_manifest"], *:contains("Binary Manifests")',
            run: 'click',
        },
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
            run: () => {
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
            content: "Click away to commit text fields before auto-save",
            trigger: '.o_form_sheet',
            run: 'click',
        },
        {
            content: "Explicitly save the record to decouple input from RPC execution and unhide the action button",
            trigger: '.o_form_button_save',
            run: 'click',
        },
        {
            content: "Click Install Now using immutable name attribute",
            trigger: 'button[name="action_install"]',
            run: 'click',
        },
        {
            content: "Wait for the success notification to ensure the RPC resolved to prevent Dirty Form crash",
            trigger: '.o_notification:contains("Success")',
            run: () => {},
        },
        {
            trigger: 'body',
            run: () => {},
        }
    ],
});
