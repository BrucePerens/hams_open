/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

/*
 * [@ANCHOR: COMM_edge_cache_csrf_refresh]
 * Tests [@ANCHOR: COMM_test_edge_cached_form_posts_after_token_fetch]
 *
 * A public page may come from Cloudflare's edge cache, rendered for an earlier visitor, and the
 * server sends such a page without a session cookie (cloudflare/models/ir_http.py), so the CSRF
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
 */

const TOKEN_URL = "/cloudflare/csrf_token";
const TOKEN_FORM = 'form:has(input[name="csrf_token"]), .s_website_form';
const WEBSITE_FORM_SEND = ".s_website_form_send, .o_website_form_send";

let pending = null;
let settled = false;

function applyToken(token) {
    globalThis.odoo.csrf_token = token;
    for (const meta of document.querySelectorAll('meta[name="csrf_token"]')) {
        meta.setAttribute("value", token);
    }
    for (const input of document.querySelectorAll('input[name="csrf_token"]')) {
        input.value = token;
    }
}

export function refreshCsrfToken() {
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
                console.warn("cloudflare: could not refresh the CSRF token", error);
            })
            .finally(() => {
                settled = true;
            });
    }
    return pending;
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
    if (settled || !(form instanceof HTMLFormElement) || !form.matches(TOKEN_FORM)) {
        return;
    }
    ev.preventDefault();
    ev.stopImmediatePropagation();
    const submitter = ev.submitter && ev.submitter.form === form ? ev.submitter : null;
    refreshCsrfToken().then(() => form.requestSubmit(submitter));
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

export function startEdgeCacheCsrfRefresh() {
    const sessionInfo = globalThis.odoo && globalThis.odoo.__session_info__;
    if (!sessionInfo || !sessionInfo.is_website_user) {
        return false;
    }
    document.addEventListener("focusin", onEarlyInteraction, true);
    document.addEventListener("pointerdown", onEarlyInteraction, true);
    document.addEventListener("submit", onSubmit, true);
    document.addEventListener("click", onWebsiteFormSend, true);
    return true;
}

startEdgeCacheCsrfRefresh();
