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

    // [@ANCHOR: COMM_test_edge_cache_csrf_token_fetched_again_at_submit]
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
});
