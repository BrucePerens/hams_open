/** @odoo-module **/

import { Interaction } from "@web/public/interaction";
import { registry } from "@web/core/registry";

/**
 * ManualTOC Widget
 * * Adheres strictly to the project mandate forbidding jQuery in new frontend assets.
 * Uses modern OWL Interaction pattern.
 * Uses Vanilla JS to dynamically scan the article body for headings and
 * generates a sticky Table of Contents.
 */
export class ManualTOC extends Interaction {
    static selector = '.o_manual_body';

    start() {
        const tocContainer = document.getElementById('manual_toc_container');
        if (!tocContainer) {
            return;
        }

        // [@ANCHOR: manual_toc_logic]
        // See story_manual_toc and journey_user_browsing
        // Verified by [@ANCHOR: test_tour_manual_toc]
        // Scan only the manual body for relevant headings
        const headings = this.el.querySelectorAll('h2, h3');
        if (headings.length === 0) {
            return;
        }

        tocContainer.innerHTML = '';

        const header = document.createElement('h5');
        header.className = "text-uppercase text-muted fs-6 tracking-wide mb-3 border-bottom pb-2";
        header.textContent = "On this page";
        tocContainer.appendChild(header);

        const ul = document.createElement('ul');
        ul.className = "nav flex-column";

        headings.forEach((heading, index) => {
            const id = heading.id || 'toc-heading-' + index;
            heading.id = id;

            const levelClass = heading.tagName.toLowerCase() === 'h2' ? 'ps-0 fw-bold' : 'ps-3';

            const li = document.createElement('li');
            li.className = "nav-item";

            const a = document.createElement('a');
            a.className = `nav-link text-muted py-1 ${levelClass}`;
            a.href = `#${id}`;
            a.textContent = heading.textContent;

            li.appendChild(a);
            ul.appendChild(li);
        });

        tocContainer.appendChild(ul);
    }
}
registry.category("public.interactions").add("knowledge.ManualTOC", ManualTOC);
