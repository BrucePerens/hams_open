/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// # Verified by [@ANCHOR: test_helpdesk_operator_tour]
registry.category("web_tour.tours").add("helpdesk_operator_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        {
            trigger: '[data-menu-xmlid="hams_helpdesk.menu_hams_helpdesk_root"]',
            content: "Open the Helpdesk app",
            run: "click",
        },
        {
            trigger: '.o_list_button_add',
            content: "Create a new ticket",
            run: "click",
        },
        {
            trigger: 'div[name="name"] input',
            content: "Fill in the ticket subject",
            run: "edit Emergency Core Router Failure",
        }
    ].concat(TourUtils.safeSave()).concat([
        {
            trigger: 'button[name="action_shift_handoff"]',
            content: "Initiate the Shift Handoff procedure",
            run: "click",
        },
        TourUtils.waitForElement('.modal-content', 'Wait for Handoff Modal to mount and render'),
        {
            trigger: 'div[name="handoff_notes"] textarea',
            content: "Enter briefing notes for the next operator",
            run: "edit I have verified the power supply. Awaiting NOC confirmation.",
        },
        {
            trigger: '.modal-footer button[name="action_confirm_handoff"]',
            content: "Confirm the handoff to transfer ownership",
            run: "click",
        }
    ]),
});
