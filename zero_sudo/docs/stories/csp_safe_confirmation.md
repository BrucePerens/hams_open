<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Confirmation Dialogs That Survive a Strict Content-Security-Policy

As a **User** about to erase or lock my account, I want the "are you sure?" step to appear even
on a site whose Content-Security-Policy forbids inline scripts.

## Background

An inline `onsubmit="return confirm(...)"` runs only when script-src allows `'unsafe-inline'`; a
CSP nonce never authorizes an inline event handler. hams_com's `content_security_policy` module
drops `'unsafe-inline'`, so under it those forms submitted with no confirmation at all.

## The Process

A form opts in with `data-hams-confirm="<message>"`. The script is an ordinary external file shipped in `web.assets_frontend`
(`zero_sudo/static/src/js/confirm_submit.js`), so it is governed by `script-src 'self'` and works whether or not any CSP is in
force. The attribute name differs on purpose from hams_com's own `data-csp-confirm`, so a page never gets two dialogs when both
modules are installed.

### The delegated `submit` listener `[@ANCHOR: zero_sudo:confirm_submit_delegated_listener]`

| Input | Meaning |
|---|---|
| `submit` event on `document` | fires for every form on the page, including forms added after load, because the listener is delegated and not bound per form |
| `data-hams-confirm` on the form | the message passed to the dialog; the text is only ever given to `window.confirm`, never inserted as HTML |

The listener finds the form with `event.target.closest("form[data-hams-confirm]")`. A form without the attribute is not touched.
With it, the listener calls `window.confirm(message)` and, when the answer is "no", calls `event.preventDefault()` so the form is
not submitted. Returns nothing and raises nothing; the default (confirmed) path lets the submit continue unchanged. It runs in
the browser only, with no server call. `window.confirm` is looked up when the event fires, not captured at load, so a tour that
replaces it (`bypassDialogs()` in `tour_utils.js`) still controls the answer. Used by the account-erasure and account-lockout
forms, where an inline `onsubmit="return confirm(...)"` would be silently skipped under a strict CSP (a nonce never authorises
an inline event handler) and the irreversible action would run with no "are you sure?" step.

## Verification

`zero_sudo/tests/test_confirm_submit_csp.py` checks the script is in the frontend bundle and that
the erasure and lockout forms carry the attribute instead of an inline handler.
