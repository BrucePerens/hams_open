/** @odoo-module **/

// pager_check_views.xml's form has ~15 fields whose `invisible=` condition
// is keyed off `check_type` -- a real "Complex State Machine" per ADR 0076
// section 1, which MANDATES a tour here, not just `audit-ignore-view`. This
// tour exercises the actual dynamic behavior (a field appearing/
// disappearing when check_type changes), not just static form-filling --
// that's the whole reason the ADR requires it: a DOM-based tour is the only
// thing that actually proves the invisible-state logic renders correctly,
// the way a backend Python test covering field values never would.
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("pager_check_tour", {
    url: "/odoo?debug=1",
    steps: () => [
        { trigger: "body", content: "Initialize Tour" },
        {
            trigger: ".o_navbar_apps_menu button",
            content: "Open apps menu",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="pager_duty.menu_admin_root"]',
            content: "Open Pager Duty app",
            run: "click",
        },
        {
            trigger: '[data-menu-xmlid="pager_duty.menu_pager_checks"]',
            content: "Go to Monitoring Checks",
            run: "click",
        },
        {
            trigger: ".o_list_button_add, .o-kanban-button-new",
            content: "Create a new monitoring check",
            run: "click",
        },
        {
            trigger: '.o_field_widget[name="name"] input',
            content: "Enter check name",
            run: "edit Tour Test Check",
        },
        {
            trigger: '.o_field_widget[name="check_type"] .o_select_menu_toggler',
            content: "Open Monitor Type dropdown",
            run: "click",
        },
        {
            // See incident_tour.js's own comment: Odoo 19's SelectMenu
            // component renders no data-value attribute, only
            // data-choice-index, so matching has to go by the rendered
            // label text via a custom run() rather than a CSS selector.
            trigger: ".o_select_menu_item",
            content: "Select HTTP(S) Endpoint",
            run: function () {
                const item = Array.from(document.querySelectorAll(".o_select_menu_item")).find((el) =>
                    el.textContent.includes("HTTP(S) Endpoint")
                );
                if (!item) {
                    throw new Error('Could not find a .o_select_menu_item containing "HTTP(S) Endpoint".');
                }
                item.click();
            },
        },
        {
            // Tests [@ANCHOR: COMM_pager_check_dynamic_invisible_http]
            // check_type=http makes `target` visible (it's only invisible
            // for 'heartbeat') -- this is the actual dynamic-state
            // assertion, not just filling in a field that was always there.
            trigger: '.o_field_widget[name="target"] input',
            content: "Target field must be visible for an HTTP(S) Endpoint check -- fill it in",
            run: "edit https://example.invalid/health",
        },
        {
            trigger: '.o_field_widget[name="payload_expect"] input',
            content: "payload_expect must also be visible for check_type=http",
            run: "edit 200",
        },
        {
            trigger: '.o_field_widget[name="check_type"] .o_select_menu_toggler',
            content: "Switch Monitor Type to Heartbeat",
            run: "click",
        },
        {
            trigger: ".o_select_menu_item",
            content: "Select Heartbeat (Push Monitor)",
            run: function () {
                const item = Array.from(document.querySelectorAll(".o_select_menu_item")).find((el) =>
                    el.textContent.includes("Heartbeat (Push Monitor)")
                );
                if (!item) {
                    throw new Error('Could not find a .o_select_menu_item containing "Heartbeat (Push Monitor)".');
                }
                item.click();
            },
        },
        {
            // Tests [@ANCHOR: COMM_pager_check_dynamic_invisible_heartbeat]
            // The real assertion this tour exists for: switching to
            // check_type='heartbeat' must make `target` disappear (it's
            // explicitly invisible for this type) and reveal the
            // "Heartbeat Info" notebook page -- proving the invisible-state
            // machine actually reacts to the field change, not just that
            // it renders once on load. Odoo 19's native tour triggers use
            // querySelectorAll, which has no :contains() equivalent
            // (jQuery-only, crashes instantly per incident_tour.js's own
            // comment) -- match tab text via a custom run() instead.
            // Confirmed directly against the real Odoo 19 source
            // (form_compiler.js): an invisible field isn't rendered with a
            // hidden class, it's compiled OUT of the template entirely when
            // its own `invisible` modifier is true -- so the correct check
            // is that '.o_field_widget[name="target"]' is absent from the
            // DOM, not that it carries some invisibility class.
            trigger: ".o_notebook_headers",
            content: "Heartbeat Info tab must appear once check_type is Heartbeat",
            run: function () {
                const tab = Array.from(document.querySelectorAll(".o_notebook_headers .nav-link")).find((el) =>
                    el.textContent.includes("Heartbeat Info")
                );
                if (!tab) {
                    throw new Error('Expected a "Heartbeat Info" tab to appear once check_type=heartbeat, but none was found.');
                }
                if (document.querySelector('.o_field_widget[name="target"]')) {
                    throw new Error(
                        "Expected the 'target' field to be absent from the DOM once check_type=heartbeat, but it is still present."
                    );
                }
            },
        },
        {
            trigger: ".o_form_sheet",
            content: "Click away to force DOM blur and commit text input",
            run: "click",
        },
        { trigger: ".o_form_sheet:not(.o_dirty)", run: function () {} },
    ].concat(TourUtils.safeSave()),
});
