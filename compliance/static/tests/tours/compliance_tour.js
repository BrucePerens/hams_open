/** @odoo-module **/
// -*- coding: utf-8 -*-
// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: AGPL-3.0-or-later
import { registry } from "@web/core/registry";

// Tests [@ANCHOR: COMM_test_compliance_ui_tour]
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/en_US/privacy?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        // Remove the website cookies bar before it can auto-show -- it's
        // itself a Bootstrap .modal injected into website.layout, and
        // Odoo's tour engine picks the LAST visible .modal in DOM order
        // for its "is this click below a modal" safety check
        // (web_tour/static/src/js/tour_automatic/tour_step_automatic.js),
        // so once it appears it can out-rank an unrelated open modal
        // elsewhere on the page. This tour doesn't open any modal of its
        // own, but removing the bar here is still correct: nothing here
        // depends on it, and leaving it in place is a standing risk for
        // whatever gets added to this tour next.
        {
            trigger: 'body',
            content: 'Remove the website cookies bar before it can auto-show',
            run: function() {
                document.querySelector('#website_cookies_bar')?.remove();
            }
        },
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
        { trigger: 'body', run: function() {} },
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
        { trigger: 'body', run: function() {} },
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
        { trigger: 'body', run: function() {} },
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
        { trigger: 'body', run: function() {} },
        {
            trigger: ".o_tour_compliance_doc_link",
            content: 'Verify Compliance Documents link is present',
        },
    ],
});
