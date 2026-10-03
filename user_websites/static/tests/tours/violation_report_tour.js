/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";


// Both tours below run as an anonymous visitor, so the modal must ask for the guest's email
// address (report_violation_modal renders it only for the public user). Fail loudly when the
// field is missing rather than skipping it: an "if (email)" guard here once hid that the field
// never rendered at all, and every guest report was stored without its reporter's address.
// Tests [@ANCHOR: report_violation_guest_email_field]
const fillGuestEmail = (address) => ({
    trigger: '#reportViolationModal input[name="email"]',
    content: "Fill in the guest email address the form asks an anonymous visitor for",
    run: `edit ${address}`,
});

// Shared by both tours below: open the report modal, fill it in, submit it.
const reportViolationSteps = (beforeSubmit = [], description = "Unsolicited advertising links.") => [
        { trigger: 'body', content: 'Initialize Tour' },
        TourUtils.bypassDialogs(),
        {
            trigger: "body",
            content: "Dismiss the website cookies bar if it appears, so it can't out-rank #reportViolationModal in Odoo's tour engine's \"last visible modal\" check",
            run: function () {
                // #website_cookies_bar (website/views/website_templates.xml)
                // auto-shows ~500ms after load (data-show-after="500") and
                // is itself a Bootstrap .modal, injected into website.layout
                // AFTER #reportViolationModal. web_tour's own
                // elementIsInModal safety check (tour_step_automatic.js)
                // picks the LAST visible .modal in DOM order and requires
                // the click target to be inside *that* one -- so once the
                // cookies bar appears, every click inside our real modal
                // fails with "It is not allowed to do action on an element
                // that's below a modal", even though the cookies bar has no
                // backdrop (s_popup_no_backdrop) and covers nothing on
                // screen. Confirmed directly: a temporary diagnostic dump of
                // every .modal element mid-tour showed exactly this --
                // #reportViolationModal and #website_cookies_bar both
                // "show" simultaneously, cookies bar last in DOM order.
                // .js_close_popup is the real, tested close handler
                // (website/static/src/interactions/popup/popup.js calls
                // bsModal.hide() from it) -- not a guess. The #modal DOM
                // node exists from first render, just hidden behind
                // .d-none/.o_snippet_invisible on an ancestor -- clicking
                // .js_close_popup before the bar has actually shown (i.e.
                // before its own data-show-after timer fires) does NOT
                // reliably suppress that later timer, confirmed empirically
                // (a first attempt that clicked as soon as the DOM node
                // existed, instead of waiting for it to genuinely show,
                // still hit the exact same failure on a real rerun). So
                // this polls for the modal to actually carry .show before
                // clicking, then waits for Bootstrap's own hidden.bs.modal
                // event to confirm the dismissal genuinely completed --
                // same rigor as the shown.bs.modal wait below. Resolves
                // either way: some deployments have website.cookies_bar
                // disabled entirely (the bar's own template is
                // t-if="website.cookies_bar"), so the modal node never
                // existing at all is a legitimate outcome too.
                const barModal = document.querySelector("#website_cookies_bar .modal");
                if (!barModal) {
                    return;
                }
                return new Promise((resolve) => {
                    let settled = false;
                    const finish = () => {
                        if (!settled) {
                            settled = true;
                            resolve();
                        }
                    };
                    barModal.addEventListener("hidden.bs.modal", finish, { once: true });
                    const deadline = Date.now() + 2500;
                    const poll = setInterval(() => {
                        if (barModal.classList.contains("show")) {
                            clearInterval(poll);
                            const closeBtn = barModal.querySelector(".js_close_popup");
                            if (closeBtn) {
                                closeBtn.click();
                            } else {
                                finish();
                            }
                        } else if (Date.now() > deadline) {
                            clearInterval(poll);
                            finish();
                        }
                    }, 100);
                });
            },
        },
        {
            trigger: 'button[data-bs-target="#reportViolationModal"]',
            content: "Open violation reporting modal, deterministically waiting for Bootstrap's fade-in transition to finish before continuing",
            run: function () {
                // #reportViolationModal has class="modal fade" -- Bootstrap
                // adds .show immediately to *start* the opacity/backdrop
                // transition, not once it's done, so polling for .show or
                // for the textarea's mere presence in the DOM can win the
                // race while the modal is still fading in and its backdrop
                // is still capturing clicks intended for elements inside
                // it ("It is not allowed to do action on an element that's
                // below a modal"). shown.bs.modal is Bootstrap's own event
                // for "transition genuinely complete" -- listener has to be
                // attached before the click that triggers show(), or a
                // fast-enough transition (e.g. prefers-reduced-motion) could
                // fire and be missed before this step's own listener exists.
                document.getElementById("reportViolationModal").addEventListener(
                    "shown.bs.modal",
                    () => document.body.classList.add("o_report_violation_modal_shown"),
                    { once: true }
                );
                document.querySelector('button[data-bs-target="#reportViolationModal"]').click();
            },
        },
        {
            trigger: "body.o_report_violation_modal_shown",
            content: "Wait for the modal's fade-in transition to genuinely finish",
            run: function () {},
        },
        {
            trigger: 'textarea[name="description"]',
            content: "Provide description notes",
            run: `edit ${description}`,
        },
        { trigger: '.modal-body', content: 'Blur form to commit state', run: 'click' },
        ...beforeSubmit,
        {
            trigger: 'button[type="submit"].btn-danger',
            content: "Submit violation ticket and trigger page reload",
            run: "click",
            expectUnloadPage: true,
        },
        {
            trigger: 'body',
            content: 'Wait for page reload after successful controller redirect',
            run: () => {}
        }
];

// Tests [@ANCHOR: user_websites:UX_REPORT_VIOLATION]
registry.category("web_tour.tours").add("test_tour_violation_report", {
    steps: () => reportViolationSteps([fillGuestEmail("normal-page-tour@example.com")]),
});

// Tests [@ANCHOR: COMM_edge_cache_csrf_refresh]
// The same report, filed from a page the server marked edge-cacheable and sent without a session
// cookie, so the CSRF token in the form belongs to no one. cloudflare's edge_cache_csrf.js must
// fetch a fresh token (and the session it is bound to) before the form posts; without it the post
// is rejected with a 400 and never redirects back with report_submitted=1.
registry.category("web_tour.tours").add("test_tour_violation_report_edge_cached", {
    steps: () => [
        ...reportViolationSteps([
            fillGuestEmail("edge-cache-tour@example.com"),
        ], "Edge-cached page report, test_11"),
        // The controller answers an accepted report with a redirect to ...?report_submitted=1, but
        // toast_notifications.js (UrlToastNotification) shows its success toast and then strips that
        // parameter with history.replaceState as soon as the page starts, so document.location no
        // longer carries it by the time this step runs. The success toast is the lasting evidence; a
        // rejected post (400, or ?error=...) never shows it. test_11 also checks the created row.
        TourUtils.waitForText(
            "We received your report",
            "The report was accepted: the success toast for report_submitted=1 is shown"
        ),
    ],
});
