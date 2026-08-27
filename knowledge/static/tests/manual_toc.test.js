/** @odoo-module **/

// start() reads document.getElementById('manual_toc_container') directly
// and builds the TOC from this.el's real h2/h3 children -- exercised
// against a real DOM fixture (hoot tests run in a real browser), not a
// faked document, since the function's whole job is real DOM traversal
// and construction.
import { afterEach, describe, expect, test } from "@odoo/hoot";
import { ManualTOC } from "@knowledge/js/manual_toc";

function makeFixture(bodyHtml) {
    const container = document.createElement("div");
    container.id = "manual_toc_container";
    document.body.appendChild(container);

    const articleBody = document.createElement("div");
    articleBody.innerHTML = bodyHtml;
    document.body.appendChild(articleBody);

    return { container, articleBody };
}

describe("manual_toc", () => {
    describe.current.tags("knowledge_manual_toc");

    afterEach(() => {
        document.getElementById("manual_toc_container")?.remove();
    });

    test("start() does nothing when the TOC container is missing from the page", () => {
        const instance = Object.create(ManualTOC.prototype);
        const articleBody = document.createElement("div");
        articleBody.innerHTML = "<h2>Section</h2>";
        instance.el = articleBody;
        // Must not throw even with no #manual_toc_container anywhere.
        instance.start();
    });

    test("start() does nothing (leaves the container untouched) when the body has no h2/h3 headings", () => {
        const { container, articleBody } = makeFixture("<p>No headings here.</p>");
        const instance = Object.create(ManualTOC.prototype);
        instance.el = articleBody;

        instance.start();

        expect(container.innerHTML).toBe("");
    });

    test("start() builds a nav list from real h2/h3 headings, assigning ids to headings that lack one", () => {
        const { container, articleBody } = makeFixture(`
            <h2>Getting Started</h2>
            <h3 id="already-has-id">Prerequisites</h3>
            <h2>Advanced Usage</h2>
        `);
        const instance = Object.create(ManualTOC.prototype);
        instance.el = articleBody;

        instance.start();

        const links = container.querySelectorAll("a.nav-link");
        expect(links.length).toBe(3);
        expect(links[0].textContent).toBe("Getting Started");
        expect(links[0].className).toInclude("fw-bold"); // h2 -> bold, ps-0
        expect(links[1].textContent).toBe("Prerequisites");
        expect(links[1].getAttribute("href")).toBe("#already-has-id"); // pre-existing id preserved
        expect(links[1].className).toInclude("ps-3"); // h3 -> indented, not bold
        expect(links[2].getAttribute("href")).toBe("#toc-heading-2"); // no id -> generated from its index
    });

    test("start() clears any stale TOC content already in the container before rebuilding", () => {
        const { container, articleBody } = makeFixture("<h2>Only Section</h2>");
        container.innerHTML = "<p>Stale content from a previous render</p>";
        const instance = Object.create(ManualTOC.prototype);
        instance.el = articleBody;

        instance.start();

        expect(container.textContent).not.toInclude("Stale content");
        expect(container.querySelectorAll("a.nav-link").length).toBe(1);
    });
});
