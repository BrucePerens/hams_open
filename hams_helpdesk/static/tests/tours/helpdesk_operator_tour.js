/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// # Verified by [@ANCHOR: test_helpdesk_operator_tour]
registry.category("web_tour.tours").add("helpdesk_operator_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        // Wait for the home menu app icon to render using pure structure-agnostic attributes
        { trigger: '[data-menu-xmlid="hams_helpdesk.menu_hams_helpdesk_root"]', content: 'Wait for Helpdesk App Icon on Root Menu', run: function() {} },
        {
            trigger: '[data-menu-xmlid="hams_helpdesk.menu_hams_helpdesk_root"]',
            content: "Open the Helpdesk app",
            run: "click",
        },
        // Wait for the list view to load before trying to click add
        { trigger: '.o_list_button_add', content: 'Wait for Create Button', run: function() {} },
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
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        }
    ].concat(TourUtils.safeSave()).concat([
        { trigger: 'button[name="action_shift_handoff"]', content: 'Wait for Handoff button', run: function() {} },
        {
            trigger: 'button[name="action_shift_handoff"]',
            content: "Initiate the Shift Handoff procedure",
            run: "click",
        },
        { trigger: '.modal-body', content: 'Wait for Handoff Modal to mount and render', run: function() {} },
        {
            trigger: 'div[name="handoff_notes"] textarea',
            content: "Enter briefing notes for the next operator",
            run: "edit I have verified the power supply. Awaiting NOC confirmation.",
        },
        {
            trigger: '.modal-body',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        },
        {
            trigger: 'button[name="action_confirm_handoff"]',
            content: "Confirm the handoff to transfer ownership",
            run: "click",
        }
    ]),
});
