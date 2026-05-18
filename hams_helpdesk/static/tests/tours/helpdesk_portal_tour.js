/** @odoo-module **/
import { registry } from "@web/core/registry";

registry.category("web_tour.tours").add("helpdesk_portal_tour", {
    url: "/my/tickets",
    steps: () => [
        {
            trigger: 'table tbody tr td a',
            content: "Click on the first available ticket in the list",
            run: "click",
            expectUnloadPage: true,
        },
        {
            trigger: '#optional_classes.container h3',
            content: "Verify that the ticket detail page loaded successfully",
        },
        {
            trigger: '.o_portal_chatter',
            content: "Verify the chatter is available",
        }
    ],
});
