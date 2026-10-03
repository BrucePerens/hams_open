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

1. A form opts in with `data-hams-confirm="<message>"`. One delegated `submit` listener on
   `document`, shipped as an ordinary script in `web.assets_frontend`, asks `window.confirm` with
   that message and cancels the submit on "no". It covers forms added after page load, and looks
   `window.confirm` up at submit time so tours can still answer it
   `[@ANCHOR: zero_sudo:confirm_submit_delegated_listener]`.

## Verification

`zero_sudo/tests/test_confirm_submit_csp.py` checks the script is in the frontend bundle and that
the erasure and lockout forms carry the attribute instead of an inline handler.
