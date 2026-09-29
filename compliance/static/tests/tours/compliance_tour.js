/** @odoo-module **/
// -*- coding: utf-8 -*-
// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: AGPL-3.0-or-later
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Tests [@ANCHOR: COMM_test_compliance_ui_tour]
// Real bug found live 2026-09-29 running the full test suite: this tour used to reimplement its
// own cookie-bar removal (removing #website_cookies_bar by id, on a MutationObserver), instead of
// the shared TourUtils.dismissCookiesBar() macro every other tour in this codebase already uses.
// That local version missed the actual failure mode -- #website_cookies_bar's own Bootstrap modal
// is a nested, id-less `.modal.o_cookies_discrete` child that gets reparented directly under
// <body>, independent of its wrapper, non-deterministically relative to page load (more installed
// modules -> more JS -> more likely to trigger it; this tour alone never reproduced it, but it did
// reliably once other modules were also in the mix). Removing only the wrapper by id does nothing
// for the already-reparented inner element once that happens, so web_tour's own "last visible
// modal" safety check then refuses every subsequent step ("not allowed to do action on an element
// that's below a modal"). TourUtils.dismissCookiesBar() (zero_sudo/static/src/js/tour_utils.js)
// already handles exactly this -- watching for and removing the reparented element specifically,
// not just the wrapper -- and was already adopted by every other tour in this codebase after being
// independently rediscovered on ham_propagation's and ham_testing's own tours. This tour was the
// one holdout still using its own, incomplete, reimplementation.
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/en_US/privacy?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        TourUtils.dismissCookiesBar(),
        {
            trigger: 'h1',
            content: 'Verify Privacy Policy content',
            run: function () {
                const text = document.body.textContent;
                if (!text.includes('Privacy Policy') || !text.includes('Disclaimer: This document is provided') || !text.includes('Data Minimization')) {
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
