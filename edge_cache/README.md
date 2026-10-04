# Edge Cache (`edge_cache`)

*Copyright (c) HAMS project. Licensed under AGPL-3.0-or-later.*

Lets Cloudflare's edge cache serve an Odoo website's public pages and asset bundles. Depends on `website`
only: no Redis, no `zero_sudo`, so a small tenant instance (perens.com, postopen.org) can install it. The
`cloudflare` module depends on it and adds the Cloudflare API, purging, WAF and tunnel management.

## What it does

* **Assets.** `/web/assets/*` answers 200/304 get `Cloudflare-CDN-Cache-Control: max-age=31536000` and the
  cache tag `odoo-static-assets`, with no `Set-Cookie`. Anything else under that path is `no-store`.
* **Pages.** A website page is offered to the edge only when Odoo's own page cache accepts it
  (`website.page._allow_to_use_cache`: GET, no query string, public user, not group-restricted), the request
  carried no `session_id` and no `website_cookies_bar` cookie, the status is 200/304, and the response ends up
  with no `Set-Cookie`. It gets `max-age=86400` and the tag `odoo-website-<id>`.
* **Cookies.** Odoo gives every cookieless visitor a new unsaved `session_id` and a default-language
  `frontend_lang` cookie. On an edge-cacheable GET both carry nothing for that visitor, so both are dropped.
  A cached response is replayed to everyone, so a response never carries a cookie and is cacheable at once.
* **Never cached:** `/web/`, `/odoo`, `/my/`, `/api/`, `/shop/cart|checkout|confirm_order`, `/helpdesk/`, any
  POST, any logged-in visitor, any response with a cookie that is not one of the two above.
* **Forms.** The CSRF token baked into a cached page belongs to a session nobody holds. A small script
  (`static/src/js/edge_cache_csrf.js`) fetches a fresh token from `/cloudflare/csrf_token` (the route keeps its
  old path) before a form on such a page posts; that response sets the visitor's session cookie.
* **Page cache.** `website.page` hands the dispatcher a copy of a freshly rendered cache entry so one
  visitor's `Set-Cookie` is never stored in Odoo's page cache and replayed to the next.

## Cloudflare side (dashboard, not code)

Two rules: bypass the cache for any request carrying a `session_id` cookie (first), then cache by the
`Cloudflare-CDN-Cache-Control` header. See `night_shift_questions/open/cloudflare-html-cache-rules-go-ahead-*`
in hams_com. Without the rules the headers do nothing.

## Not done here

A visitor's own `X-Forwarded-Host` header is removed by `cloudflare/wsgi_proxy_scheme.py`, which needs Redis. A
tenant instance running `proxy_mode` without the `cloudflare` module still trusts that header; strip it in
the tunnel before turning HTML caching on for a tenant.
