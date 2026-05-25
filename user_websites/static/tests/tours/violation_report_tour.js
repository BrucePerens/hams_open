/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@hams_test/js/tour_utils";

// Tests [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
registry.category("web_tour.tours").add("test_tour_violation_report", {
    url: "/",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: 'a[data-bs-target="#reportViolationModal"]',
            content: "Open violation reporting modal",
            run: "click",
        },
        {
            trigger: '.o_select_menu[name="reason"]',
            content: "Click to open the custom Odoo 19 select dropdown menu",
            run: "click",
        },
        {
            trigger: '.o_select_menu_item',
            content: "Select the specific menu option item",
            run: function () {
                const items = document.querySelectorAll('.o_select_menu_item');
                for (const item of items) {
                    if (item.textContent.includes('Spam')) {
                        item.click();
                        break;
                    }
                }
            }
        },
        {
            trigger: 'textarea[name="description"]',
            content: "Provide description notes using correct Odoo 19 input simulator",
            run: "edit Unsolicited advertising links.",
        },
        {
            trigger: 'button[type="submit"].btn-danger',
            content: "Submit violation ticket and trigger page reload",
            run: "click",
            expectUnloadPage: true,
        },
        {
            trigger: 'body',
            content: 'Wait for page reload after successful controller redirect',
            run: function() {}
        }
    ]
});
