/** @odoo-module **/
// -*- coding: utf-8 -*-
// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: AGPL-3.0-or-later
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Tests [@ANCHOR: COMM_test_compliance_ui_tour]
// This tour uses the shared TourUtils.dismissCookiesBar() macro rather than its own cookie-bar
// removal (removing #website_cookies_bar by id, on a MutationObserver), because the id-based
// removal misses the actual failure mode -- #website_cookies_bar's own Bootstrap modal
// is a nested, id-less `.modal.o_cookies_discrete` child that gets reparented directly under
// <body>, independent of its wrapper, non-deterministically relative to page load (more installed
// modules -> more JS -> more likely to trigger it; it reproduces reliably only with other
// modules installed). Removing only the wrapper by id does nothing
// for the already-reparented inner element once that happens, so web_tour's own "last visible
// modal" safety check then refuses every subsequent step ("not allowed to do action on an element
// that's below a modal"). TourUtils.dismissCookiesBar() (zero_sudo/static/src/js/tour_utils.js)
// already handles exactly this -- watching for and removing the reparented element specifically,
// not just the wrapper -- and is used by every other tour in this codebase.
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/en_US/privacy?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        TourUtils.dismissCookiesBar(),
        {
            trigger: 'h1',
            content: 'Verify Privacy Policy content',
            run: function () {
                // The default wording (draft disclaimer, "Data Minimization") belongs to the template this module ships and is
                // asserted against its own arch by test_pages_content. A site module may legitimately replace that wording on
                // the live page (ham_init publishes hams.com's own), so this step checks the page a visitor sees is a policy
                // with its sections, not the template's exact sentences.
                const text = document.body.textContent;
                if (!text.includes('Privacy Policy') || document.querySelectorAll('#wrap h3').length < 3) {
                    throw new Error('[!] DIAGNOSTIC FOR AI: Privacy Policy content missing.');
                }
            }
        },
        // Check footer links
        {
            trigger: ".o_tour_footer_privacy",
            content: 'Verify Privacy Policy link in footer',
        },
        {
            trigger: ".o_tour_footer_cookie_policy",
            content: 'Verify Cookie Policy link in footer',
        },
        {
            trigger: ".o_tour_footer_terms",
            content: 'Verify Terms of Service link in footer',
        },
        {
            trigger: ".o_tour_footer_accessibility",
            content: 'Verify Accessibility Statement link in footer',
        },
        {
            trigger: ".o_tour_footer_my_privacy",
            content: 'Verify My Privacy link in footer',
        },
        // Navigate to Cookie Policy
        {
            trigger: ".o_tour_footer_cookie_policy",
            content: 'Click on Cookie Policy link in footer',
            run: 'click',
            expectUnloadPage: true,
        },
        TourUtils.dismissCookiesBar(),
        {
            trigger: 'h1',
            content: 'Verify Cookie Policy page loaded',
            run: function () {
                if (!document.body.textContent.includes('Cookie Policy')) {
                    throw new Error('[!] DIAGNOSTIC FOR AI: Cookie Policy page failed to load.');
                }
            }
        },
        // Navigate to Terms of Service
        {
            trigger: ".o_tour_footer_terms",
            content: 'Click on Terms of Service link in footer',
            run: 'click',
            expectUnloadPage: true,
        },
        TourUtils.dismissCookiesBar(),
        {
            trigger: 'h1',
            content: 'Verify Terms of Service page loaded',
            run: function () {
                if (!document.body.textContent.includes('Terms of Service')) {
                    throw new Error('[!] DIAGNOSTIC FOR AI: Terms of Service page failed to load.');
                }
            }
        },
        // Navigate to Accessibility Statement
        {
            trigger: ".o_tour_footer_accessibility",
            content: 'Click on Accessibility Statement link in footer',
            run: 'click',
            expectUnloadPage: true,
        },
        TourUtils.dismissCookiesBar(),
        {
            trigger: 'h1',
            content: 'Verify Accessibility Statement page loaded',
            run: function () {
                const text = document.body.textContent;
                if (!text.includes('Accessibility Statement') || !text.includes('WCAG 2.1 level AA')) {
                    throw new Error('[!] DIAGNOSTIC FOR AI: Accessibility Statement page failed to load or content missing.');
                }
            }
        },
        // Navigate to Compliance Index
        {
            trigger: "body",
            content: "Inject Compliance Index link",
            run: function () {
                const link = document.createElement('a');
                link.href = '/compliance';
                link.className = 'temp_compliance_nav_link';
                link.textContent = 'Compliance';
                document.body.appendChild(link);
            }
        },
        {
            trigger: ".temp_compliance_nav_link",
            content: "Navigate to Compliance Index",
            run: 'click',
            expectUnloadPage: true,
        },
        TourUtils.dismissCookiesBar(),
        {
            trigger: ".o_tour_compliance_doc_link",
            content: 'Verify Compliance Documents link is present',
        },
    ],
});
