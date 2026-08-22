/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";


// [@ANCHOR: test_tour_moderation_appeal]

// Tests [@ANCHOR: UX_SUBMIT_APPEAL]
registry.category("web_tour.tours").add("moderation_appeal_tour", {
    url: "/my/home",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: 'form[action="/website/submit_appeal"] textarea[name="reason"]',
            content: "Wait for the suspension alert's appeal form to render",
            run: function () {},
        },
        {
            trigger: 'form[action="/website/submit_appeal"] textarea[name="reason"]',
            content: "Provide an appeal explanation",
            run: "edit The reported content was a misclassification -- please review the original post.",
        },
        {
            trigger: 'form[action="/website/submit_appeal"] button[type="submit"]',
            content: "Submit the appeal and trigger a page reload",
            run: "click",
            expectUnloadPage: true,
        },
        TourUtils.waitForElement(
            '.o_notification_manager .o_notification',
            "success toast pushed to the DOM after a real appeal submission"
        ),
        {
            trigger: 'body',
            content: "Verify the form is replaced by the 'reviewing your appeal' state now that a 'new' appeal exists (existing_appeal in user_websites_templates.xml)",
            run: function () {
                if (document.querySelector('form[action="/website/submit_appeal"]')) {
                    throw new Error(
                        "Appeal form is still showing after a successful submission -- " +
                        "existing_appeal should have suppressed it."
                    );
                }
            },
        },
    ],
});
