/** @odoo-module **/
import { registry } from "@web/core/registry";

// # Verified by [@ANCHOR: test_compliance_ui_tour]
registry.category("web_tour.tours").add("compliance_tour", {
    url: "/privacy?debug=1",
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
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
        // Navigate to Cookie Policy
        {
            trigger: "footer a[href='/cookie-policy']",
            content: 'Click on Cookie Policy link in footer',
            run: 'click',
            expectUnloadPage: true,
        },
        {
            trigger: 'body',
            content: 'Wait for page load after navigation to Cookie Policy',
            run: function() {}
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
            trigger: 'body',
            content: 'Wait for page load after navigation to Terms of Service',
            run: function() {}
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
            trigger: 'body',
            content: 'Wait for page load after navigation to Accessibility Statement',
            run: function() {}
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
