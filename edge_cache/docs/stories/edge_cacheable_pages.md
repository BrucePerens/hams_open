# Story: Public pages and assets cacheable at the edge

As a **site owner on a small Odoo instance behind Cloudflare**, I want my public pages and asset bundles
cached at Cloudflare's edge without per-visitor cookies on them, so that most visits never reach the server.

The server side is in `edge_cache/models/ir_http.py` (the response headers), `edge_cache/models/website_page.py` (which pages are offered) and `edge_cache/controllers/csrf_token.py`; the browser side is `static/src/js/edge_cache_csrf.js`. Nothing here gates or rejects a request: the module only adds caching directives, so direct access works the same with or without Cloudflare. Tests are in `tests/` and `static/tests/`.

## Scenario: An anonymous visitor reads a page

### Strip redundant cookies [@ANCHOR: COMM_cloudflare_strip_redundant_cookies]
1. The first visit to a plain website page sets no cookie, because the visitor's empty session and default language carry nothing worth keeping. `IrHttp._cloudflare_strip_redundant_cookies(response)` (a classmethod, returns `None`, changes the response's `Set-Cookie` list in place) drops two cookies: `session_id` when the request brought none and the session was never persisted (no uid, not dirty, no rotation due), and `frontend_lang` when the request brought none and its value is the website's default language code. Any other cookie is kept, which makes the response per-visitor and so uncacheable. Test: `test_edge_cacheable_page_has_no_set_cookie`.

### Edge-cacheable decision [@ANCHOR: COMM_cloudflare_edge_cacheable]
`IrHttp._cloudflare_edge_cacheable(response) -> bool` decides whether an opted-in, already dispatched response may be cached at the edge. All of these must hold: the method is GET or HEAD, the status is 200 or 304, the user is the public user, the request carried neither a `session_id` nor a `website_cookies_bar` cookie, and after the redundant cookies are stripped no `Set-Cookie` remains. It has the side effect of stripping those cookies. Tests: `test_edge_cacheable_page_has_no_set_cookie` and `test_login_page_and_post_targets_are_never_cacheable`.

### Page opt-in [@ANCHOR: COMM_website_page_edge_cache_opt_in]
2. Only pages Odoo's own page cache accepts are offered. `WebsitePage._get_response(request)` asks Odoo's `_allow_to_use_cache` (a GET, no query string, the public user, no group-restricted page) and, when true, sets the internal header `X-Cloudflare-Edge-Cacheable: 1` on the response; `ir.http._post_dispatch` makes the final decision and removes the header before the response leaves. It returns the response (or a falsy value unchanged). Test: `test_edge_cacheable_page_has_no_set_cookie`.

### Routes never cached [@ANCHOR: cf_nocache_routes]
A page for a visitor who already has a session or a consent choice, a page with a query string, a logged-in visitor's page and every backend, API and cart route are never cacheable. In `_post_dispatch` a path starting with `/my/`, `/odoo`, `/web/`, `/api/`, `/shop/cart`, `/shop/checkout`, `/shop/confirm_order` or `/helpdesk/` gets `Cloudflare-CDN-Cache-Control: no-cache, no-store`, and `/web/image` and `/web/content` are deliberately included so private attachments never leak through the edge cache. Tests in `test_edge_cache_headers.py` and `test_login_page_and_post_targets_are_never_cacheable`.

### Asset bundles and header injection [@ANCHOR: COMM_ir_http_post_dispatch_headers]
3. The asset bundles are cached for a year, only when the response is a success with no cookie. `IrHttp._post_dispatch(response)` (a classmethod; it calls `super()` first and returns its result) reads and always removes the opt-in marker, marks error answers never stored, and for `/web/assets` sets `Cloudflare-CDN-Cache-Control: max-age=31536000` plus `Cache-Tag: odoo-static-assets` only for status 200 or 304 with no `Set-Cookie`, otherwise `no-cache, no-store`. Opted-in public pages get `max-age=86400` and a `Cache-Tag` of `odoo-website-<id>` when a website is known. Without a request it returns unchanged. Tests: `test_edge_cache_headers.py`.

### Never share a cache entry [@ANCHOR: COMM_website_page_no_shared_cache_entry]
4. One visitor's cookie must never reach the next. When Odoo's page cache re-renders an expired entry it
   stores that very response object and returns it, so everything done to the response afterwards (this
   visitor's session cookie, a rewritten body) would change the stored entry. The page handler therefore hands
   the dispatcher a copy (a new `http.Response` with copied headers, mimetype, status and body list, and the same `qcontext`) of a freshly rendered response and leaves the cache entry intact. Cache hits, flagged by `_post_process_response_from_cache`, are not copied. Test: `test_page_cache_never_replays_a_set_cookie`.

### No inherited Set-Cookie [@ANCHOR: COMM_website_page_no_inherited_set_cookie]
5. As a second line of defence, any `Set-Cookie` already present on a page response at that point can only have
   come from a cache entry, since a page's own cookies are added later from the future response, so `_get_response` clears the header list with `setlist("Set-Cookie", [])` on every page response, cacheable or not. Test: `test_page_cache_never_replays_a_set_cookie`.

## Scenario: A deploy mistake answers 404 or 500

### Errors are never stored [@ANCHOR: COMM_cloudflare_error_never_stored]
6a. Every error answer is marked `Cache-Control: no-store`, so a cached error page cannot keep a site down after the fix is deployed. `IrHttp._cloudflare_error_never_stored(response) -> bool` replaces the `Cache-Control` of every status 400 or above with `no-store` and returns true; for a redirect, 304 or success it changes nothing and returns false. `_post_dispatch` calls it for every response. Test: `test_08_error_helper_replaces_long_max_age_and_spares_success_and_redirects`.

### Error pages built by Odoo [@ANCHOR: COMM_ir_http_handle_error_no_store]
`IrHttp._handle_error(exception)` marks the error page Odoo renders (404, 403, 500) as never stored at the source, because a `NotFound` raised from a controller does not pass through `_post_dispatch`: a normal `Response` gets `no-store` and `Cloudflare-CDN-Cache-Control: no-cache, no-store`; a bare werkzeug exception of status 400 or above is wrapped in a WSGI callable that builds the same body with those headers. Anything else is returned unchanged. Test: `test_07_error_responses_are_no_store`.

### Test for error answers [@ANCHOR: COMM_test_error_responses_are_no_store]
`test_07_error_responses_are_no_store` in `tests/test_edge_cache_headers.py` requests a missing route, a missing blog post and a stale `/web/assets/1/...` bundle with `allow_redirects=False`, and asserts each answer is 400 or more with `Cache-Control: no-store` and `Cloudflare-CDN-Cache-Control: no-cache, no-store`. It needs the installed module and changes only the test database.

## Scenario: A visitor posts a form on a cached page

### Client-side token refresh [@ANCHOR: COMM_edge_cache_csrf_refresh]
6. The page's CSRF (cross-site request forgery) token belongs to no session. Before the form posts, a script fetches a fresh token and
   the session cookie it is bound to. `edge_cache_csrf.js` runs only for anonymous website visitors (`session.is_website_user`); it starts the fetch lazily when the visitor first focuses or presses a token form, replays a held submit once the token is applied, and fetches again at every submit, because a late `session_id` from another cookieless request can replace the cookie an earlier token was bound to. A failed fetch only logs a warning and the form posts with the token it has. Tests: `edge_cache_csrf.test.js`, `test_edge_cached_form_posts_after_token_fetch`.

### Token route [@ANCHOR: COMM_cloudflare_csrf_token_route]
`CloudflareCsrfToken.csrf_token(self)`, route `GET /cloudflare/csrf_token`, `auth="public"`, `readonly=True`, no arguments. Returns JSON `{"csrf_token": <token>}` from `request.csrf_token(None)` (the same lifetime as the page's own token) with `Cache-Control: no-store`; the response is never edge-cacheable and carries the visitor's session cookie, so token and cookie arrive together. It writes nothing but the session cookie. Tests: `test_edge_cached_form_posts_after_token_fetch`, `test_late_session_cookie_of_another_cookieless_request_voids_an_earlier_token`.

### Token-form detection [@ANCHOR: COMM_edge_cache_is_token_form]
`isTokenForm(el)` in `edge_cache_csrf.js` is true for an element matching `.s_website_form` or an HTML form containing an `input[name="csrf_token"]`. It is written without the CSS `:has()` selector, so a browser that lacks it (Chrome before 105) does not throw a `SyntaxError` from `closest()` or `matches()` at every focus and submit on the page, which the capture listeners on `document` would otherwise do. Pure, no side effects. Test: the `:has()` test below.

### Test without `:has()` [@ANCHOR: COMM_test_edge_cache_csrf_works_without_has_selector]
The Hoot test "a browser without the :has() selector still gets the token, and nothing throws" in `static/tests/edge_cache_csrf.test.js` makes `closest` and `matches` throw a `SyntaxError` for any selector containing `:has(`, focuses a field and clicks Send, and asserts that the form still posts with the freshly fetched token `fresh`. The prototypes are restored afterwards. It runs in the browser test runner and changes only the test page.

**Status:** Verified by the three tests below.

### Cacheable page has no cookie [@ANCHOR: COMM_test_edge_cacheable_page_has_no_set_cookie]
`test_edge_cacheable_page_has_no_set_cookie` in `tests/test_edge_cache_cookies.py` makes a first anonymous request to a plain page and asserts the response is marked edge-cacheable and has no `Set-Cookie` header; it needs the installed module and changes only the test database.

### Cached form posts [@ANCHOR: COMM_test_edge_cached_form_posts_after_token_fetch]
`test_edge_cached_form_posts_after_token_fetch` loads a cacheable page (`max-age=86400`) and posts `/website/form` with the token baked into it, asserting HTTP 400 because that token belongs to a session nobody holds. With a fresh visitor it then fetches `/cloudflare/csrf_token`, asserts 200, `no-store` and a `session_id` cookie, posts with that token and asserts 200, and finally asserts that a visitor holding a session no longer gets edge-cacheable pages. It needs the installed module and changes only the test database.

### Page cache never replays a cookie [@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]
`test_page_cache_never_replays_a_set_cookie` fills Odoo's page cache, then sets `_CACHE_DURATION` to -1 so that visitor A (no session, but a consent cookie, so nothing strips A's new session cookie) forces a re-render, and asserts visitor B, who already holds a session, gets a cache hit with status 200 and never receives A's `session_id`. The 200 also proves a rewritten body was not stored as the cached entry. It covers the copy of a fresh render, the removal of an inherited `Set-Cookie`, and the cache-hit flags set by `_get_response_raw` and `_post_process_response_from_cache`. It changes only the test database.
