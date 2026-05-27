/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

registry.category("web_tour.tours").add("helpdesk_portal_tour", {
    url: "/my/tickets",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        { trigger: 'table tbody tr td a', content: 'Wait for Ticket List to Render', run: function() {} },
        TourUtils.clickAndUnload('table tbody tr td a'),
        { trigger: '#optional_classes.container h3', content: 'Wait for: Verify that the ticket detail page loaded successfully', run: function() {} },
        { trigger: '.o_portal_chatter', content: 'Wait for: Verify the chatter is available', run: function() {} },

    ],
});
