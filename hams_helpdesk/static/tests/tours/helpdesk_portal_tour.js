/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// # Verified by [@ANCHOR: test_helpdesk_portal_tour]

registry.category("web_tour.tours").add("helpdesk_portal_tour", {
    url: "/my/tickets",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        { trigger: 'table tbody tr td a', content: 'Wait for Ticket List to Render', run: function() {} },
        {
            trigger: 'table tbody tr td a',
            content: 'Click ticket to navigate to detail page',
            run: 'click',
            expectUnloadPage: true, // Navigation breaks SPA
        },
        { trigger: 'body', content: 'Wait for page unload and hydrate', run: function() {} },
        // Verify we are on the ticket detail page using a robust native structural class
        { trigger: '.o_portal_chatter', content: 'Wait for: Verify the chatter is available', run: function() {} },
    ],
});
