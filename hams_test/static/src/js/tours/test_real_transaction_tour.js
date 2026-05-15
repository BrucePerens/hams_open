/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("test_real_transaction_tour", {
    url: "/web",
    steps: () => [
        // # Verified by [@ANCHOR: test_noisy_table_tour]
        {
            trigger: '[data-menu-xmlid="base.menu_administration"]',
            content: "Open Settings",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="base.menu_custom"]',
            content: "Open Technical Menu",
            run: "click",
        },
        {
            trigger: 'a.o_menu_entry_lvl_2[data-menu-xmlid="test_real_transaction.menu_noisy_table"]',
            content: "Open Noisy Tables",
            run: "click",
        },
        {
            trigger: ".o_list_button_add",
            content: "Click Create",
            run: "click",
        },
        {
            trigger: ".o_tour_trigger_noisy_table_name_form input",
            content: "Enter table name",
            run: "text tour_test_table",
        },
        {
            trigger: ".o_form_button_save",
            content: "Save",
            run: "click",
        },
    ],
});
