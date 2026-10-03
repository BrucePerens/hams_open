/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

/*
 * [@ANCHOR: zero_sudo:confirm_submit_delegated_listener]
 *
 * CSP-safe replacement for `<form onsubmit="return confirm('...');">` on server-rendered pages.
 *
 * An inline event-handler attribute (`onsubmit=`, `onclick=`, ...) runs only when the page's
 * Content-Security-Policy script-src allows `'unsafe-inline'`. A CSP nonce never authorizes one:
 * nonce-source applies to `<script>`/`<style>` elements only
 * (https://www.w3.org/TR/CSP3/#grammardef-nonce-source). The hams_com `content_security_policy`
 * module drops `'unsafe-inline'` from script-src on every response, so under it an inline
 * `onsubmit` confirmation is silently skipped and the form submits with no confirmation at all --
 * for the account-erasure and account-lockout forms, that is an irreversible action with no
 * "are you sure?" step.
 *
 * Usage: put `data-hams-confirm="<message>"` on the `<form>`. A deliberately different attribute
 * name from hams_com's own `data-csp-confirm` (content_security_policy/static/src/js/
 * csp_unobtrusive_handlers.js), so a page never gets two dialogs when both modules are installed.
 *
 * This is an ordinary external script in web.assets_frontend, governed by script-src's 'self',
 * and it works the same whether or not any CSP is in force. One delegated listener on `document`
 * covers every form on every page, including forms added after load. `window.confirm` is looked
 * up when the event fires, not captured at load, so a tour that replaces it
 * (zero_sudo/static/src/js/tour_utils.js bypassDialogs()) still controls the answer.
 */
document.addEventListener("submit", (event) => {
    const form = event.target instanceof Element && event.target.closest("form[data-hams-confirm]");
    if (form && !window.confirm(form.getAttribute("data-hams-confirm"))) {
        event.preventDefault();
    }
});
