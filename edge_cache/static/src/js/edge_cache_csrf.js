/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

/*
 * [@ANCHOR: COMM_edge_cache_csrf_refresh]
 * Verified by [@ANCHOR: COMM_test_edge_cached_form_posts_after_token_fetch]
 * Verified by [@ANCHOR: COMM_test_edge_cache_csrf_token_fetched_again_at_submit]
 *
 * A public page may come from Cloudflare's edge cache, rendered for an earlier visitor, and the
 * server sends such a page without a session cookie (edge_cache/models/ir_http.py), so the CSRF
 * token baked into it (odoo.csrf_token, <meta name="csrf_token">, <input name="csrf_token">) is
 * bound to a session nobody holds. Before a form that carries a token posts, fetch a fresh one
 * from /cloudflare/csrf_token, which also hands the visitor the session cookie it is bound to.
 *
 * Lazy on purpose: a visitor who only reads keeps no session cookie, so the next page they open
 * can still come from the edge cache. The fetch starts when the visitor first focuses or presses
 * on a field or button of such a form; a token-bearing form submit, or a website-form "send"
 * click, that comes before the fetch finishes is held and replayed once it settles. (Odoo's
 * website form checks CSRF only for logged-in sessions, so for it this is belt and braces.) Anonymous visitors only: a
 * logged-in visitor's pages are never edge-cached, so their token is already good.
 *
 * The token is fetched again at every submit of a token-bearing form, not only the first time. A
 * token is bound to the session cookie the browser holds, and a cookieless page load fires several
 * requests at once (/sw.js, /website/translations, ...), each answered with a session_id cookie of
 * its own: the last response to arrive wins. One that is slow (a loaded server answered /sw.js after
 * 3.4 s) can land after the early fetch and replace the cookie the earlier token was bound to, and
 * the post is then refused with "CSRF validation failed". A token fetched with the cookie as it is
 * at the moment of the submit cannot go stale that way.
 */

import { session } from "@web/session";

const TOKEN_URL = "/cloudflare/csrf_token";
const TOKEN_FORM = 'form:has(input[name="csrf_token"]), .s_website_form';
const WEBSITE_FORM_SEND = ".s_website_form_send, .o_website_form_send";

let pending = null;
let settled = false;
// Forms whose submit event is the replay of one this script held, after a token was fetched for it.
const replaying = new WeakSet();

function applyToken(token) {
    globalThis.odoo.csrf_token = token;
    for (const meta of document.querySelectorAll('meta[name="csrf_token"]')) {
        meta.setAttribute("value", token);
    }
    for (const input of document.querySelectorAll('input[name="csrf_token"]')) {
        input.value = token;
    }
}

function fetchToken() {
    if (!pending) {
        pending = fetch(TOKEN_URL, {
            credentials: "same-origin",
            cache: "no-store",
            headers: { Accept: "application/json" },
        })
            .then((response) => {
                if (!response.ok) {
                    throw new Error(`${TOKEN_URL} answered ${response.status}`);
                }
                return response.json();
            })
            .then((data) => applyToken(data.csrf_token))
            .catch((error) => {
                // Submit anyway with the token the page carries; if it is stale the server
                // answers with its ordinary CSRF error, the same as before this script existed.
                console.warn("edge_cache: could not refresh the CSRF token", error);
            })
            .finally(() => {
                settled = true;
                pending = null;
            });
    }
    return pending;
}

/** The early fetch: once per page, so a visitor who only focuses a field is not asked again. */
export function refreshCsrfToken() {
    return settled && !pending ? Promise.resolve() : fetchToken();
}

function tokenForm(target) {
    return target instanceof Element ? target.closest(TOKEN_FORM) : null;
}

function onEarlyInteraction(ev) {
    if (tokenForm(ev.target)) {
        refreshCsrfToken();
    }
}

function onSubmit(ev) {
    const form = ev.target;
    if (!(form instanceof HTMLFormElement) || !form.matches(TOKEN_FORM)) {
        return;
    }
    if (replaying.has(form)) {
        return;
    }
    ev.preventDefault();
    ev.stopImmediatePropagation();
    const submitter = ev.submitter && ev.submitter.form === form ? ev.submitter : null;
    fetchToken().then(() => {
        // requestSubmit() fires the submit event before it returns, so the mark is seen by this
        // handler and then removed, even when the form's own validation stops the submit.
        replaying.add(form);
        try {
            form.requestSubmit(submitter);
        } finally {
            replaying.delete(form);
        }
    });
}

function onWebsiteFormSend(ev) {
    const button = ev.target instanceof Element ? ev.target.closest(WEBSITE_FORM_SEND) : null;
    if (settled || !button) {
        return;
    }
    ev.preventDefault();
    ev.stopImmediatePropagation();
    refreshCsrfToken().then(() => button.click());
}

/** Installs the listeners on an anonymous visitor's page; returns a function that removes them, or null. */
export function startEdgeCacheCsrfRefresh() {
    // @web/session, not odoo.__session_info__: that module consumes and deletes the global when it
    // loads, which is before this one in the bundle, so the global is gone by the time this runs.
    if (!session.is_website_user) {
        return null;
    }
    document.addEventListener("focusin", onEarlyInteraction, true);
    document.addEventListener("pointerdown", onEarlyInteraction, true);
    document.addEventListener("submit", onSubmit, true);
    document.addEventListener("click", onWebsiteFormSend, true);
    return () => {
        document.removeEventListener("focusin", onEarlyInteraction, true);
        document.removeEventListener("pointerdown", onEarlyInteraction, true);
        document.removeEventListener("submit", onSubmit, true);
        document.removeEventListener("click", onWebsiteFormSend, true);
    };
}

startEdgeCacheCsrfRefresh();
