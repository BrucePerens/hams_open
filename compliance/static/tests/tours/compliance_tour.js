/** @odoo-module **/
// -*- coding: utf-8 -*-
// Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General
// Public License v3.0 (AGPL-3.0).
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Tests [@ANCHOR: test_compliance_ui_tour]
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/en_US/privacy?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        // Handle Cookie Bar if it appears
        {
            trigger: 'body',
            content: 'Check for Cookie Bar',
            run: function() {
                const cookieBarBtn = document.querySelector('.js_close_cookie_bar, #website_cookies_bar .btn-primary');
                if (cookieBarBtn) {
                    cookieBarBtn.click();
                }
                const cookieBar = document.querySelector('#website_cookies_bar');
                if (cookieBar) {
                    cookieBar.remove(); // Force remove to bypass animation
                }
            }
        },
        {
            trigger: 'h1',
            content: 'Verify Privacy Policy content',
            run: function () {
                const text = document.body.textContent;
                if (!text.includes('Privacy Policy') || !text.includes('Warning: This is the default') || !text.includes('Data Minimization')) {
                    throw new Error('[!] DIAGNOSTIC FOR AI: Privacy Policy content missing.');
                }
            }
        },
        // Check footer links
        {
            trigger: "footer a[href='/privacy']",
            content: 'Verify Privacy Policy link in footer',
        },
        {
            trigger: "footer a[href='/cookie-policy']",
            content: 'Verify Cookie Policy link in footer',
        },
        {
            trigger: "footer a[href='/terms']",
            content: 'Verify Terms of Service link in footer',
        },
        {
            trigger: "footer a[href='/accessibility']",
            content: 'Verify Accessibility Statement link in footer',
        },
        {
            trigger: "footer a[href='/my/privacy']",
            content: 'Verify My Privacy link in footer',
        },
        // Navigate to Cookie Policy
        {
            trigger: "footer a[href='/cookie-policy']",
            content: 'Click on Cookie Policy link in footer',
            run: 'click',
            expectUnloadPage: true,
        },
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
            trigger: "footer a[href='/terms']",
            content: 'Click on Terms of Service link in footer',
            run: 'click',
            expectUnloadPage: true,
        },
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
            trigger: "footer a[href='/accessibility']",
            content: 'Click on Accessibility Statement link in footer',
            run: 'click',
            expectUnloadPage: true,
        },
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
    ],
});
