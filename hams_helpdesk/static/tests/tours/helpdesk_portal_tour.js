/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("helpdesk_portal_tour", {
    url: "/my/tickets",
    steps: () => [
        {
            trigger: 'table tbody tr td a',
            content: "Click on the first available ticket in the list",
            run: "click",
        },
        {
            trigger: '#optional_classes.container h3',
            content: "Verify that the ticket detail page loaded successfully",
            run: () => {
                if (!document.querySelector('#optional_classes.container h3')) {
                    console.error("Ticket detail header not found.");
                }
            },
        },
        {
            trigger: 'textarea[name="message"]',
            content: "Verify the chatter box is available for customer replies",
            run: () => {
                console.log("Portal tour completed successfully.");
            }
        }
    ],
});
