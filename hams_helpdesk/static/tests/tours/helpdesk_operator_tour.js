/** @odoo-module **/
import { registry } from "@web/core/registry";

// # Verified by [@ANCHOR: test_helpdesk_operator_tour]
registry.category("web_tour.tours").add("helpdesk_operator_tour", {
    url: "/odoo",
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
        },
        {
            trigger: '.o_form_button_save',
            content: "Save the ticket",
            run: "click",
        },
        {
            trigger: 'button[name="action_shift_handoff"]',
            content: "Initiate the Shift Handoff procedure",
            run: "click",
        },
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
    ],
});
