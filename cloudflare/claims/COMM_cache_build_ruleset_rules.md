---
anchor: cloudflare:COMM_cache_build_ruleset_rules
code_hash: sha256:88bcfc6a95192c8d9f5baf5dfb51ce5b120c121da5430a262d921e9874c45925
hash_algorithm: v2
review_tier: 1
---

# Claim: `_build_cache_ruleset_rules`

`_build_cache_ruleset_rules(website)` turns the active `cloudflare.cache.rule` rows of the website (or of no website) into Cloudflare `set_cache_settings` rule dicts (at most 1000 rows). Bypass rules come first whatever their sequence; because Cloudflare applies every matching cache rule, each cache rule's expression is also ANDed with the negation of every bypass expression. A positive `edge_cache_ttl` overrides the origin's edge TTL; otherwise the origin's TTLs are respected. It reads Odoo only.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.
