# Journey: Intelligent Traffic Handling

This journey illustrates how edge metadata is used for dynamic application logic. Full interfaces: [Geo-Aware Request Context](../stories/request_context.md) and [CAPTCHA Verification with Turnstile](../stories/turnstile_verification.md).

## Phase 1: Edge Enrichment
- Visitor hits the Cloudflare Edge.
- Cloudflare adds geo-location and threat headers to the request.

### The system injects caching headers via `_post_dispatch` [@ANCHOR: edge_cache:COMM_ir_http_post_dispatch_headers]

`ir.http._post_dispatch(response)` in the `edge_cache` module runs on every outgoing response and adds `Cloudflare-CDN-Cache-Control` (the edge's own directive). `/web/assets` bundles get `max-age=31536000` and the tag `odoo-static-assets`; technical and per-user routes (`/odoo`, `/web/`, `/my/`, `/api/`, shop cart and checkout, `/helpdesk/`) get `no-cache, no-store`; a public page gets `max-age=86400` and the tag `odoo-website-<id>` only if the page opted in, the request is a GET or HEAD by the public user with no session or cookie-consent cookie, and the response sets no cookie. Error answers always get `Cache-Control: no-store`. The tags are what the purge queue's tag purges target. It returns the response and never blocks a request.

## Phase 2: Context Extraction

### The application calls `get_request_context()` [@ANCHOR: COMM_cf_get_request_context]

`env['cloudflare.utils'].get_request_context()` returns a dict of the visitor's address and the edge-supplied geography and threat score for the current HTTP request (an empty dict outside a request). It takes no arguments and changes nothing. The `CF-*` headers are believed only when the connecting peer is trusted; otherwise the geography and threat values are `None`.

- Trusted headers are parsed into a clean dictionary. "Trusted" means the `CF-*` headers are used only when the connecting peer is loopback or in the trusted Cloudflare IP range list; from any other peer the geographic and threat fields come back as `None` (see Edge Context in the [module README](../../README.md)).

## Phase 3: Application Response

### The controller decides whether to require Turnstile verification [@ANCHOR: COMM_cf_turnstile_verify]

The controller uses the context (for example `threat_score`, a string such as `"10"`, so convert it before comparing) to decide whether to ask the visitor for a Turnstile token, then calls `env['cloudflare.turnstile'].verify_token(token, remote_ip, website_id)`. It returns `True` only when Cloudflare's siteverify endpoint says the token is valid for the website's secret, `False` for a missing secret or token, an invalid token or a network failure. The secret is never returned to the caller, so anonymous visitors may trigger it.

- Content is served based on the visitor's `country`, which is `None` when the peer was not trusted: the controller must have a default.

**Status:** Verified by the two tests below.

### `test_01_get_request_context_via_loopback_tunnel_trusts_cf_headers` [@ANCHOR: COMM_test_cf_get_request_context]

Mocks the Odoo request with a loopback peer (127.0.0.1) and a set of `CF-*` headers (connecting IP, country US, city, longitude, latitude, threat score 10), calls `get_request_context()` and asserts the returned `ip`, `country`, `city` and `threat_score` match the headers. A sibling test shows that the same headers from a non-loopback peer give `None` for every edge field.

### `test_02_turnstile_secret_fetch` [@ANCHOR: COMM_test_cf_turnstile_verify]

Stores a Turnstile secret on the website, invalidates the cache so the next read must really decrypt it (an earlier version of the test passed by reading its own cached write), mocks the Cloudflare HTTP post to answer `{"success": true}`, calls `verify_token("fake_token_123", "odoo", website_id=...)` and asserts `True`, and that the siteverify request carried the decrypted secret as `secret` and the visitor token as `response`. A companion test, `test_02b_verify_token_works_for_an_unprivileged_caller`, repeats the call as a bare portal user and asserts a real verdict instead of an `AccessError`.
