/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("helpdesk_operator_tour", {
    url: "/odoo?debug=1&action=hams_helpdesk.action_hams_helpdesk_ticket",
    steps: () => [
        {
            content: "Click Create Ticket",
            trigger: '.o_list_button_add',
            run: 'click',
        },
        {
            content: "Fill Subject",
            trigger: 'div[name="name"] input',
            run: 'edit Operator Tour Ticket',
        },
        {
            content: "Click away to blur",
            trigger: '.o_form_sheet',
            run: 'click',
        },
    ].concat(TourUtils.safeSave()).concat([
        {
            content: "Click Shift Handoff",
            trigger: 'button[name="action_shift_handoff"]',
            run: 'click',
        },
        {
            content: "Wait for Wizard",
            trigger: '.modal-body',
            run: function() {},
        },
        {
            content: "Select New User",
            trigger: 'div[name="new_user_id"] input',
            run: 'click',
        },
        {
            content: "Pick first user",
            trigger: '.o-autocomplete--dropdown-item:first',
            run: 'click',
        },
        {
            content: "Force Blur for Wizard before confirmation",
            trigger: '.modal-body',
            run: 'click',
        },
        {
            content: "Fill Notes",
            trigger: 'div[name="handoff_notes"] textarea',
            run: 'edit Handing off for the night.',
        },
        {
            content: "Force Blur for Notes",
            trigger: '.modal-body',
            run: 'click',
        },
        {
            content: "Confirm Handoff",
            trigger: 'button[name="action_confirm_handoff"]',
            run: 'click',
        },
        {
            content: "Wait for Wizard to close",
            trigger: 'body:not(:has(.modal))',
            run: function() {},
        },
        {
            content: "Verify Handoff in Chatter",
            trigger: 'body',
            run: function() {
                return new Promise((resolve, reject) => {
                    let interval = setInterval(() => {
                        const thread = document.querySelector('.o_mail_thread');
                        if (thread && thread.textContent.includes("Official Shift Handoff Executed")) {
                            clearInterval(interval);
                            resolve();
                        }
                    }, 250);
                    setTimeout(() => {
                        clearInterval(interval);
                        reject(new Error("Handoff message not found in chatter after wizard closed."));
                    }, 10000);
                });
            },
        }
    ])
});
