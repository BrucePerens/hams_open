/* This software is distributed under the terms of the Affero General Public License (AGPL-3). */
// [@ANCHOR: COMM_helpdesk_portal_tour]
/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("helpdesk_portal_tour", {
    url: "/my/tickets?debug=1",
    steps: () => [
        {
            content: "Click on New Ticket",
            trigger: '.o_tour_new_ticket',
            run: 'click',
            expectUnloadPage: true,
        },
        {
            content: "Fill Subject",
            trigger: 'input[name="name"]',
            run: 'edit Tour Ticket',
        },
        {
            content: "Fill Callsign",
            trigger: 'input[name="callsign"]',
            run: 'edit K1AAA',
        },
        {
            content: "Fill Description",
            trigger: 'textarea[name="description"]',
            run: 'edit This is a ticket created by a tour.',
        },
        {
            content: "Submit Ticket",
            trigger: 'button[type="submit"]',
            run: 'click',
            expectUnloadPage: true,
        },
        {
            content: "Wait for Detail Page",
            trigger: '.breadcrumb-item.active',
            run: function() {},
        },
        {
            content: "Verify Callsign",
            trigger: '.o_helpdesk_callsign',
            run: function() {
                const callsignEl = document.querySelector('.o_helpdesk_callsign');
                if (callsignEl && callsignEl.textContent.trim() === 'K1AAA') {
                    return;
                }
                throw new Error("Callsign K1AAA not found in detail page");
            },
        },
        {
            content: "Close Ticket",
            trigger: '.o_tour_close_ticket',
            run: 'click',
            expectUnloadPage: true,
        },
        {
            content: "Verify Closed Status",
            trigger: '.o_helpdesk_status_badge:contains("Closed")',
            run: function() {}
        }
    ]
});
