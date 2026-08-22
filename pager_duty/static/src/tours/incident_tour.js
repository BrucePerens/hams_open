/** @odoo-module **/

import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("pager_duty_incident_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_navbar_apps_menu button',
            content: "Open apps menu",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="pager_duty.menu_admin_root"]',
            content: "Open Pager Duty app",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="pager_duty.menu_pager_incident"]',
            content: "Go to Incidents",
            run: "click",
        },
        {
            trigger: ".o_list_button_add, .o-kanban-button-new",
            content: "Create a new incident",
            run: "click",
        },
        {
            trigger: '[name="source"] input',
            content: "Enter incident source",
            run: "edit Manual Test",
        },
        {
            trigger: '[name="severity"] .o_select_menu_toggler',
            content: "Open severity dropdown",
            run: "click",
        },
        {
            // The SelectMenu component (Odoo 19) never renders a
            // data-value attribute on its option items -- only
            // data-choice-index -- so a [data-value*=...] selector never
            // matches. Odoo 19 native tour triggers use querySelectorAll,
            // which has no ':contains()' equivalent (jQuery-only, crashes
            // instantly) -- match on the rendered label text via a custom
            // run() instead.
            trigger: '.o_select_menu_item',
            content: "Select High severity",
            run: function () {
                const item = Array.from(document.querySelectorAll(".o_select_menu_item")).find(
                    (el) => el.textContent.includes("High")
                );
                if (!item) {
                    throw new Error('Could not find a .o_select_menu_item containing "High".');
                }
                item.click();
            },
        },
        {
            trigger: '[name="description"] textarea',
            content: "Enter description",
            run: "edit This is a manual test incident.",
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        },
        { trigger: '.o_form_sheet:not(.o_dirty)', run: function() {} }
    ].concat(TourUtils.safeSave()),
});
