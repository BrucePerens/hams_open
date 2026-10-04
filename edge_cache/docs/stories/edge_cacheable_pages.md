# Story: Public pages and assets cacheable at the edge

As a **site owner on a small Odoo instance behind Cloudflare**, I want my public pages and asset bundles
cached at Cloudflare's edge without per-visitor cookies on them, so that most visits never reach the server.

## Scenario: An anonymous visitor reads a page

1. The first visit to a plain website page is marked cacheable and sets no cookie, because the visitor's
   empty session and default language carry nothing worth keeping
   `[@ANCHOR: COMM_cloudflare_strip_redundant_cookies]` `[@ANCHOR: COMM_cloudflare_edge_cacheable]`.
2. Only pages Odoo's own page cache accepts are offered `[@ANCHOR: COMM_website_page_edge_cache_opt_in]`;
   a page for a visitor who already has a session or a consent choice, a page with a query string, a
   logged-in visitor's page and every backend, API and cart route are never cacheable
   `[@ANCHOR: cf_nocache_routes]`.
3. The asset bundles are cached for a year, only when the response is a success with no cookie
   `[@ANCHOR: COMM_ir_http_post_dispatch_headers]`.

## Scenario: A visitor posts a form on a cached page

4. The page's CSRF token belongs to no session. Before the form posts, a script fetches a fresh token and
   the session cookie it is bound to `[@ANCHOR: COMM_edge_cache_csrf_refresh]`
   `[@ANCHOR: COMM_cloudflare_csrf_token_route]`.

**Status:** Verified by `[@ANCHOR: COMM_test_edge_cacheable_page_has_no_set_cookie]`,
`[@ANCHOR: COMM_test_edge_cached_form_posts_after_token_fetch]` and
`[@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]`.
