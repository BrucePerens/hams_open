# Journey: Intelligent Traffic Handling

This journey illustrates how edge metadata is used for dynamic application logic.

## Phase 1: Edge Enrichment
- Visitor hits the Cloudflare Edge.
- Cloudflare adds geo-location and threat headers to the request.
- The system injects caching headers via `_post_dispatch` `[@ANCHOR: COMM_ir_http_post_dispatch_headers]`.

## Phase 2: Context Extraction
- The application calls `get_request_context()` `[@ANCHOR: COMM_cf_get_request_context]`.
- Trusted headers are parsed into a clean dictionary. "Trusted" means the `CF-*` headers are used only when the connecting peer is loopback or in the trusted Cloudflare IP range list; from any other peer the geographic and threat fields come back as `None` (see Edge Context in the [module README](../../README.md)).

## Phase 3: Application Response
- The controller uses the context (e.g., `threat_score`) to decide whether to require Turnstile verification `[@ANCHOR: COMM_cf_turnstile_verify]`.
- Content is served based on the visitor's `country`.

**Status:** Verified by `[@ANCHOR: COMM_test_cf_get_request_context]` and `[@ANCHOR: COMM_test_cf_turnstile_verify]`.
