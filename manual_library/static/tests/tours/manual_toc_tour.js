/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Tests [@ANCHOR: story_manual_toc]
// Tests [@ANCHOR: test_tour_manual_toc]
// Tests [@ANCHOR: manual_toc_logic]
registry.category("web_tour.tours").add("manual_toc_tour", {
    steps: () => [
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '#manual_toc_container:has(ul.nav)',
            content: 'Wait for the TOC container to render and contain a list'
        },
        {
            trigger: '#manual_toc_container a[href^="#toc-heading-"]',
            content: 'Verify that a heading link was dynamically generated'
        },
    ],
});
