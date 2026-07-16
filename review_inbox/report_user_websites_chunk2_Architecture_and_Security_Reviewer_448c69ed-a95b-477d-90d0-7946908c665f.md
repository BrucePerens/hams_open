# Architecture and Security Review: User Websites (Chunk 2)

**Reviewer:** Architecture_and_Security_Reviewer
**Focus:** Security vulnerabilities, performance bottlenecks, latency reduction, and database turn-arounds.

## 1. Executive Summary
The controllers and security definitions in `user_websites` demonstrate strict adherence to the project's Core Architecture ADRs, specifically the Zero-Sudo architecture (ADR-01) and Identity & Access Control (ADR-10). The controllers properly utilize module-level connection pooling for external services (Redis), stream large JSON payloads using Python generators (ADR-08), and validate cryptographic tokens securely using HMAC-SHA256. 

However, a performance bottleneck was identified in the `user_websites_api.py` controller which instantiates large ORM recordsets instead of directly projecting the required fields. 

## 2. Findings & Actionable Fixes

### Finding 1: Performance Bottleneck in `api_domains` (Latency Reduction)
- **File:** `hams_open/user_websites/controllers/user_websites_api.py`
- **Context:** The API endpoint `api_domains` retrieves up to 5000 `edge.routing.domain` and `ham.dns.zone` records to extract their names using `search([]).mapped("name")`. 
- **Impact:** Instantiating 5000 ORM records just to extract a single field causes unnecessary memory allocation, increased Garbage Collection overhead, and a slower database turn-around. 
- **Actionable Fix:** Replace `search().mapped()` with `search_read()`, which bypasses full ORM record instantiation and retrieves a dictionary directly from the PostgreSQL cursor.

```diff
--- a/hams_open/user_websites/controllers/user_websites_api.py
+++ b/hams_open/user_websites/controllers/user_websites_api.py
@@ -31,9 +31,9 @@
         all_domains = []
 
         # 1. Fetch edge routing domains
-        edge_domains = (
-            env_svc["edge.routing.domain"].search([], limit=5000).mapped("name")
-        )
+        edge_domains = [
+            d['name'] for d in env_svc["edge.routing.domain"].search_read([], ['name'], limit=5000)
+        ]
         all_domains.extend(edge_domains)
 
         # 2. Soft-depend on ham_dns
@@ -41,9 +41,9 @@
             try:
                 dns_env_svc = utils._get_service_env("ham_dns.user_dns_api_service")
-                zone_names = (
-                    dns_env_svc["ham.dns.zone"].search([], limit=5000).mapped("name")
-                )
+                zone_names = [
+                    d['name'] for d in dns_env_svc["ham.dns.zone"].search_read([], ['name'], limit=5000)
+                ]
                 all_domains.extend(zone_names)
             except (KeyError, ValueError) as e:   # Tested by [@ANCHOR: test_domains_api_returns_all_domains]
                 _logger.warning("Failed to fetch ham.dns.zone domains: %s", e)
```

### Finding 2: Security & Architectural Validation (Positive Findings)
- **Zero-Sudo Compliance (ADR-01):** No instances of `.sudo()` exist in `main.py` or `user_websites_api.py`. The `zero_sudo.security.utils` library successfully wraps high-privilege operations within the designated `user_websites.user_websites_service_account`.
- **Public Guest User Idiom (ADR-10):** Unauthenticated public submissions for `content.violation.report` are securely handled. `ir.model.access.csv` explicitly grants `perm_create=1` to `base.group_public`, allowing database-level Access Control rather than relying on dangerous controller escalation. 
- **Domain Sandbox Mandate (ADR-10):** The `user_websites_security.xml` successfully assigns Record Rules against `base.group_portal` and domain-specific groups without accidentally elevating community members to `base.group_user`.
- **Bounded Chunking (ADR-08):** `privacy_export` uses an elegant python generator pattern to iteratively stream `_get_gdpr_streamed_keys` as a `Response`. This guarantees an O(1) memory footprint for users with enormous datasets.
- **Connection Pooling (ADR-08):** `main.py` correctly defines `redis.ConnectionPool` at the module level rather than inside controller methods, avoiding TCP handshake exhaustion.

## 3. Next Steps
- Apply the `search_read` optimization to `user_websites_api.py`.
- No critical security vulnerabilities or architectural violations were found.
