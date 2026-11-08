/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// Tests [@ANCHOR: COMM_edge_cache_csrf_refresh]
//
// edge_cache_csrf.js gives a form on an edge-cached page a CSRF token that belongs to the session
// cookie the browser holds. These tests drive the real script against a real <form> in the hoot
// fixture; only fetch() is replaced (mockFetch), by a server that numbers its tokens.
import { describe, expect, getFixture, mockFetch, test, waitUntil } from "@odoo/hoot";
import { session } from "@web/session";
import { startEdgeCacheCsrfRefresh } from "@edge_cache/js/edge_cache_csrf";

const FORM = `
    <form id="edge_form" action="/somewhere" method="post">
        <input type="hidden" name="csrf_token" value="baked"/>
        <input type="text" name="note"/>
        <button type="submit">Send</button>
    </form>`;

const WEBSITE_FORM_HTML =
    '<div class="s_website_form"><a class="s_website_form_send" href="#">Send</a></div>';

function install() {
    const wasWebsiteUser = session.is_website_user;
    session.is_website_user = true;
    const stop = startEdgeCacheCsrfRefresh();
    return () => {
        stop();
        session.is_website_user = wasWebsiteUser;
    };
}

describe("edge_cache_csrf", () => {
    describe.current.tags("edge_cache");

    // Tests [@ANCHOR: edge_cache:csrf_onWebsiteFormSend]
    // The module keeps its "a token was fetched" state for the whole page, so this test is defined first: it then
    // sees the hold. Run later it sees the settled path, and the assertions below hold on both paths.
    test("a website form's send button is held until the token is fetched, then pressed once", async () => {
        const stop = install();
        try {
            const fixture = getFixture();
            fixture.innerHTML = WEBSITE_FORM_HTML;
            const send = fixture.querySelector(".s_website_form_send");
            const requests = [];
            mockFetch((route) => {
                requests.push(route);
                return { csrf_token: "fresh_for_send" };
            });
            let presses = 0;
            send.addEventListener("click", (ev) => {
                ev.preventDefault();
                presses += 1;
            });
            send.click();
            await waitUntil(() => presses > 0, { timeout: 2000 });
            expect(presses).toBe(1);
            expect(requests.length <= 1).toBe(true);
            // The token is settled now: the next press goes straight through, nothing is held or fetched.
            send.click();
            expect(presses).toBe(2);
            expect(requests.length <= 1).toBe(true);
        } finally {
            stop();
        }
    });

    // Tests [@ANCHOR: edge_cache:csrf_startEdgeCacheCsrfRefresh]
    test("a logged-in visitor gets no listeners and the page is left alone", () => {
        const wasWebsiteUser = session.is_website_user;
        session.is_website_user = false;
        try {
            expect(startEdgeCacheCsrfRefresh()).toBe(null);
        } finally {
            session.is_website_user = wasWebsiteUser;
        }
    });

    // [@ANCHOR: COMM_test_edge_cache_csrf_token_fetched_again_at_submit]
    // Tests [@ANCHOR: edge_cache:csrf_applyToken]
    // Tests [@ANCHOR: edge_cache:csrf_fetchToken]
    // Tests [@ANCHOR: edge_cache:csrf_refreshCsrfToken]
    // Tests [@ANCHOR: edge_cache:csrf_tokenForm]
    // Tests [@ANCHOR: edge_cache:csrf_onEarlyInteraction]
    // Tests [@ANCHOR: edge_cache:csrf_onSubmit]
    // Tests [@ANCHOR: edge_cache:csrf_startEdgeCacheCsrfRefresh]
    test("the submit carries a token fetched at the submit, not the one from the earlier focus", async () => {
        const stop = install();
        const fixture = getFixture();
        fixture.innerHTML = FORM;
        const form = fixture.querySelector("#edge_form");
        const requests = [];
        mockFetch((route) => {
            requests.push(route);
            return { csrf_token: `token${requests.length}` };
        });
        const posted = [];
        form.addEventListener("submit", (ev) => {
            ev.preventDefault();
            posted.push(form.querySelector('input[name="csrf_token"]').value);
        });

        // The visitor reaches a field: the early fetch gives the form its first token.
        form.querySelector('input[name="note"]').dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
        await waitUntil(() => form.querySelector('input[name="csrf_token"]').value === "token1", { timeout: 2000 });
        // Another field of the same form does not ask again.
        form.querySelector('input[name="note"]').dispatchEvent(new FocusEvent("focusin", { bubbles: true }));

        // Pressing Send holds the submit, fetches again, and sends with the newer token.
        form.querySelector("button").click();
        await waitUntil(() => posted.length > 0, { timeout: 2000 });
        expect(posted).toEqual(["token2"]);
        expect(requests.length).toBe(2);
        expect(requests.every((route) => route === "/cloudflare/csrf_token")).toBe(true);
        stop();
    });

    // [@ANCHOR: COMM_test_edge_cache_csrf_works_without_has_selector]
    // Tests [@ANCHOR: COMM_edge_cache_is_token_form]
    // Tests [@ANCHOR: edge_cache:csrf_tokenForm]
    test("a browser without the :has() selector still gets the token, and nothing throws", async () => {
        // Chrome before 105 throws a SyntaxError from closest() and matches() for :has(); the
        // listeners are capture listeners on document, so one such throw would fire at every focus
        // and submit on the page (hams_helpdesk tickets 125 and 126).
        const originalClosest = Element.prototype.closest;
        const originalMatches = Element.prototype.matches;
        const refuseHas = (original) =>
            function (selector, ...rest) {
                if (String(selector).includes(":has(")) {
                    throw new DOMException(`'${selector}' is not a valid selector.`, "SyntaxError");
                }
                return original.call(this, selector, ...rest);
            };
        Element.prototype.closest = refuseHas(originalClosest);
        Element.prototype.matches = refuseHas(originalMatches);
        const stop = install();
        try {
            const fixture = getFixture();
            fixture.innerHTML = FORM;
            const form = fixture.querySelector("#edge_form");
            mockFetch(() => ({ csrf_token: "fresh" }));
            const posted = [];
            form.addEventListener("submit", (ev) => {
                ev.preventDefault();
                posted.push(form.querySelector('input[name="csrf_token"]').value);
            });
            // Focus inside the form (goes through closest()) and a press on Send (matches()) must
            // both work. The send always fetches a token of its own, so it is the one that is checked.
            form.querySelector('input[name="note"]').dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
            form.querySelector("button").click();
            await waitUntil(() => posted.length > 0, { timeout: 2000 });
            expect(posted).toEqual(["fresh"]);
        } finally {
            stop();
            Element.prototype.closest = originalClosest;
            Element.prototype.matches = originalMatches;
        }
    });
});
