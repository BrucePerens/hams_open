/** @odoo-module **/
import { registry } from "@web/core/registry";

// Tests [@ANCHOR: story_manual_toc]
// Tests [@ANCHOR: test_tour_manual_toc]
// Tests [@ANCHOR: manual_toc_logic]
registry.category("web_tour.tours").add("manual_toc_tour", {
    url: "/manual",
    steps: () => [
        {
            content: "Wait for the TOC container to render",
            trigger: '#manual_toc_container ul.nav',
            run: () => {
                if (!document.querySelector('#manual_toc_container ul.nav')) {
                    console.error("TOC nav container missing");
                }
            }
        },
        {
            content: "Verify that a heading link was dynamically generated",
            trigger: '#manual_toc_container a[href^="#toc-heading-"]',
            run: () => {
                if (!document.querySelector('#manual_toc_container a')) {
                    console.error("TOC link missing");
                }
            }
        }
    ],
});
