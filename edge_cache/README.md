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
* **Errors.** Every 4xx/5xx response is sent `Cache-Control: no-store` (replacing any value it carried) and
  `Cloudflare-CDN-Cache-Control: no-cache, no-store`, so a 404 or 500 from a bad deploy cannot outlive the fix in
  a browser, Cloudflare's edge, Automatic Platform Optimization or a zone TTL rule. Redirects and 304 keep their own rules.
  `IrHttp._cloudflare_error_never_stored(response)` does it and returns whether it did
  `[@ANCHOR: COMM_cloudflare_error_never_stored]`; it runs in `_post_dispatch` and in the `_handle_error` override, because Odoo does not pass every error page it renders through `_post_dispatch` `[@ANCHOR: COMM_ir_http_handle_error_no_store]`. Not covered: the bare werkzeug 404 Odoo's asset controller raises for a missing bundle (`/web/assets/<id>/<hash>/...`, 207 bytes) reaches neither hook and carries no cache header at all (checked on hams1, 2026-11-06); see to-do `cloudflare-zone-cache-settings-and-purge-token-9c4e1b58` in hams_com.
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

## The CSRF token refresh script (`static/src/js/edge_cache_csrf.js`, ADR 0113)

Runs in the public bundle of every page. Its behaviour is described under Forms above; its functions are below. State shared by all of them lives in module variables:
`pending` (the one running token request, a `Promise`, or `null`), `settled` (`false` until the first token request has finished, successfully or not; then `true` for the rest
of the page) and `replaying` (a `WeakSet` of forms whose submit is the replay of one this script held). The constants `TOKEN_URL` (`/cloudflare/csrf_token`), `WEBSITE_FORM`,
`TOKEN_INPUT` and `WEBSITE_FORM_SEND` are the selectors and the route. Nothing is stored between page loads, nothing is sent except the token request, and no secret is logged.
Tests: `static/tests/edge_cache_csrf.test.js` (Hoot, tag `edge_cache`), run by `tests/test_edge_cache_csrf_hoot.py`.

### `[@ANCHOR: edge_cache:csrf_applyToken]` `applyToken(token)`

Purpose: puts a fresh CSRF token everywhere the page keeps one, so whichever the form or Odoo's own code reads is the new one. Called only by `fetchToken` after a good answer.
Argument `token` (string, the value the server returned): written to `odoo.csrf_token` (so `globalThis.odoo` must exist, as it does in every Odoo page), to the `value` of each
`<meta name="csrf_token">` and to each `<input name="csrf_token">` in the document. No return value. Synchronous, touches the DOM only, idempotent. Throws a `TypeError` if
`globalThis.odoo` is missing; `fetchToken` then logs it as a failed refresh.

### `[@ANCHOR: edge_cache:csrf_fetchToken]` `fetchToken()`

Purpose: gets a token from `/cloudflare/csrf_token` (which also gives the visitor the session cookie the token is bound to) and applies it. No arguments. Returns a `Promise` that
always fulfils (never rejects): a failed request (network error, status other than 2xx, a body that is not JSON) is logged with `console.warn` and the form is sent with the token
the page already carries, so the server's ordinary CSRF error is the worst outcome. While a request is running, further calls return the same `Promise` (one request at a time).
When it ends it sets `settled = true` and clears `pending`, so the next call starts a new request: a submit therefore always fetches again. Asynchronous (`fetch`, `credentials: "same-origin"`,
`cache: "no-store"`, `Accept: application/json`); no timeout of its own.

### `[@ANCHOR: edge_cache:csrf_refreshCsrfToken]` `refreshCsrfToken()`

Purpose: the early fetch, started when a visitor first reaches a form, so most visitors have a token before they press Send. No arguments. Returns a `Promise` (never rejects): the
running request when there is one, an already fulfilled one when a request has finished (`settled`), otherwise the result of `fetchToken()`. It fetches once per page; the fetch at
submit time is `onSubmit`'s. Exported.

### `[@ANCHOR: edge_cache:csrf_tokenForm]` `tokenForm(target)`

Purpose: finds the form an event belongs to, when it is one this script handles. Argument `target` (any value; normally `event.target`): anything that is not an `Element` gives
`null`. Walks from `target` up through `parentElement` and returns the first element for which `isTokenForm` is true: a `.s_website_form` element, or an `HTMLFormElement` that
contains an `input[name="csrf_token"]`. Returns that element or `null`. Uses no `:has()` selector, because a browser without it (Chrome before 105) throws from `closest()` and
`matches()`, and the listeners are capture listeners on the document (helpdesk tickets 125 and 126). Pure DOM reads.

### `[@ANCHOR: edge_cache:csrf_onEarlyInteraction]` `onEarlyInteraction(ev)`

Purpose: the listener for `focusin` and `pointerdown` (capture, on the document). Argument `ev` (`Event`): when `tokenForm(ev.target)` finds a form it calls `refreshCsrfToken()`
and ignores the answer. Other events are left alone. Never throws, never prevents the event, returns nothing.

### `[@ANCHOR: edge_cache:csrf_onSubmit]` `onSubmit(ev)`

Purpose: the listener for `submit` (capture, on the document): holds a token-bearing form's submit until a token fetched now is in the form, then sends it. Argument `ev`
(`SubmitEvent`). It returns at once, touching nothing, when the target is not a form this script handles (`isTokenForm`) or when the form is in `replaying`, which marks the replay
of a submit it held. Otherwise it calls `preventDefault()` and `stopImmediatePropagation()` (the page's own submit handlers do not see the held event; they see the replay), calls
`fetchToken()`, then marks the form in `replaying`, calls `form.requestSubmit(submitter)` (the submit button that was pressed, when it belongs to the form) and removes the mark in
the same turn, even if the form's own validation stops the submit. A failure to resubmit is logged. Asynchronous; returns nothing. The mark is what stops an endless loop.

### `[@ANCHOR: edge_cache:csrf_onWebsiteFormSend]` `onWebsiteFormSend(ev)`

Purpose: the listener for `click` (capture, on the document) for the website-form builder's Send button (`.s_website_form_send`, `.o_website_form_send`), which posts with `fetch` and
never fires a form `submit`. Argument `ev` (`MouseEvent`). Does nothing when a token has been fetched already (`settled`) or when the click is not on such a button. Otherwise it
calls `preventDefault()` and `stopImmediatePropagation()`, waits for `refreshCsrfToken()` and clicks the button again; that click finds `settled` true and goes through. Odoo's
website form checks CSRF only for a logged-in session, so for it this is belt and braces. A failure is logged. Asynchronous; returns nothing.

### `[@ANCHOR: edge_cache:csrf_startEdgeCacheCsrfRefresh]` `startEdgeCacheCsrfRefresh()`

Purpose: installs the four listeners (`focusin`, `pointerdown`, `submit`, `click`, all capture on the document). No arguments. Called once when the module loads and by the tests.
Returns a function that removes the four listeners, or `null` and installs nothing when `session.is_website_user` (from `@web/session`) is false: a logged-in visitor's pages are never
edge-cached, so their token is already good. It reads `@web/session` and not `odoo.__session_info__` because that module deletes the global when it loads, before this one runs.
Calling it twice installs the listeners twice; keep and call the returned remover first.

## Python functions (ADR 0113): cookies and page-cache flags

### `[@ANCHOR: edge_cache:set_cookie_name]` `_set_cookie_name(header_value)`

**Purpose.** Reads the cookie name out of one raw `Set-Cookie` header value, so `ir.http._cloudflare_strip_redundant_cookies` can decide whether a response sets the session cookie or the language cookie (`models/ir_http.py`).

| Argument | Type | Meaning |
| --- | --- | --- |
| `header_value` | `str` | One raw `Set-Cookie` value such as `session_id=abc; Path=/; HttpOnly`. |
| returns | `str` | Everything before the first `=` of the part before the first `;`, stripped of blanks; `""` for a value with no `=`. |

Errors: none for a `str` (a non-string raises `AttributeError`). **Special concerns.** Pure: no I/O, no state, safe to call anywhere. Attributes after the first `;` are ignored.

**See also.** `test_set_cookie_parsing` in `tests/test_edge_cache_cookies.py`.

### `[@ANCHOR: edge_cache:set_cookie_value]` `_set_cookie_value(header_value)`

**Purpose.** The twin of `_set_cookie_name`: the cookie value of one raw `Set-Cookie` header value. `_cloudflare_strip_redundant_cookies` uses it to tell a language cookie that only repeats the website's default language (dropped from the response) from one that changes it.

| Argument | Type | Meaning |
| --- | --- | --- |
| `header_value` | `str` | One raw `Set-Cookie` value. |
| returns | `str` | Everything after the first `=` of the part before the first `;`, stripped of blanks; an `=` inside the value is kept (`session_id=abc=def` gives `abc=def`); `""` for a value with no `=`. |

**Special concerns.** Pure, as `_set_cookie_name`. The value is a session identifier when the name is `session_id`: callers compare it and never log it.

**See also.** `test_set_cookie_parsing` in `tests/test_edge_cache_cookies.py`.

### `[@ANCHOR: edge_cache:website_page_get_response_raw]` `WebsitePage._get_response_raw(request)`

**Purpose.** Odoo's page cache calls `_get_response_raw` to render a page that is not in the cache. This override marks that response as a fresh render, `cf_page_cache_hit = False`, so `WebsitePage._get_response` can tell it from a cache hit and give the dispatcher a copy of it: a fresh render may be the very object Odoo stores in its cache, and the dispatcher must not add one visitor's `Set-Cookie` to it.

| Argument | Type | Meaning |
| --- | --- | --- |
| `request` | the `odoo.http` request | Passed through to Odoo unchanged. |
| returns | `http.Response` or a falsy value | What Odoo's `_get_response_raw` returned, with `cf_page_cache_hit` set to `False` when it is truthy; a falsy answer is returned untouched. |

**Special concerns.** It changes only that one attribute and does no I/O. Runs on every cache miss and on the re-render of an entry older than `_CACHE_DURATION`.

**See also.** `test_page_cache_never_replays_a_set_cookie` in `tests/test_edge_cache_cookies.py`.

### `[@ANCHOR: edge_cache:website_page_post_process_response_from_cache]` `WebsitePage._post_process_response_from_cache(request, response)`

**Purpose.** Odoo calls this on a response it built from its page cache. This override lets Odoo finish its work and then flags the response as a cache hit, `cf_page_cache_hit = True`; `WebsitePage._get_response` then knows the response is already a private copy for this request and does not copy it again.

| Argument | Type | Meaning |
| --- | --- | --- |
| `request` | the `odoo.http` request | Passed to Odoo unchanged. |
| `response` | `http.Response` | The response built from the cache; mutated in place. |
| returns | `None` | Nothing. |

**Special concerns.** It raises whatever Odoo's version raises (for example `TypeError` if the body was turned into bytes); the flag is set only after Odoo's work succeeds. No I/O and no lock.

**See also.** `test_page_cache_never_replays_a_set_cookie` in `tests/test_edge_cache_cookies.py` (the second visitor is served a cache hit).
