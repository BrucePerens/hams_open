<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Hardening the Session and Language Cookies `[@ANCHOR: zero_sudo:COMM_story_session_cookie_hardening]`

As an **Administrator**, I want the session cookie and language cookie to carry `Secure` and
`SameSite` attributes whenever the connection is genuinely HTTPS, so that neither cookie leaks over
a plain-HTTP connection or rides along on an unrelated cross-site request.

## Background

Found 2026-09-22: Odoo's own `session_id` cookie (the real authentication token) and
`frontend_lang` carried `HttpOnly` but neither `Secure` nor `SameSite`, even when the request
genuinely arrived over HTTPS via this deployment's reverse proxy.

## The Process

1. Exactly two cookie names are in scope for this hardening -- `session_id` and `frontend_lang` --
   matching that finding's own documented scope; a cookie set explicitly elsewhere with its own
   deliberate flags (e.g. `gdpr_export_token`, set by `user_websites`' `privacy_export_zip()`) is
   left untouched `[@ANCHOR: zero_sudo:hardened_cookie_names]`.
2. The actual hardening is pure string logic over one raw `Set-Cookie` header value, split out so
   it's directly unit-testable without a real HTTP request context: it adds `Secure` only when the
   request was actually HTTPS, and `SameSite=Lax` (not `Strict` -- this site's own LoTW
   passwordless-login and invite-redemption flows land back on hams.com via a cross-site redirect
   from a third party, and `Strict` would drop the session cookie on exactly that first redirected
   request) `[@ANCHOR: zero_sudo:harden_cookie_header]`.
3. `ir.http`'s own `_post_dispatch` override calls that pure logic against every `Set-Cookie` header
   on the real outgoing response, reading `request.httprequest.scheme` (already correctly reflecting
   `X-Forwarded-Proto` under this deployment's `proxy_mode=True`) to decide whether `Secure` applies
   `[@ANCHOR: zero_sudo:ir_http_post_dispatch_cookie_hardening]`.

## Verification
A real end-to-end request against `/web/login` confirms the actual `Set-Cookie` header Odoo sends
is hardened, not just the pure helper function in isolation; the pure-function cases (HTTPS vs
plain HTTP, both hardened cookie names, an unrelated cookie left alone, a cookie that already
carries its own `SameSite`) are covered directly. See `zero_sudo/tests/test_session_cookie_hardening.py`.
