/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

registry.category("web_tour.tours").add("test_real_transaction_tour", {
    url: "/odoo?action=hams_test.action_noisy_table",
    steps: () => [
        // # Verified by [@ANCHOR: test_noisy_table_tour]
        TourUtils.bypassDialogs(),
        {
            trigger: ".o_list_button_add",
            content: "Click Create",
            run: "click",
        },
        {
            trigger: ".o_tour_trigger_noisy_table_name_form input",
            content: "Enter table name",
            run: "edit tour_test_table",
        },
        ...TourUtils.safeSave(),
    ],
});
