---
anchor: user_websites:fn_site_slug_next_hop
code_hash: sha256:1ff89a492d576466ae7947b925ed3fe731dfcf6d2efa1bb0a5ea1c91045ef63b
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `_next_hop` (`user_websites/models/site_slug.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL search `website.rewrite` for the first active 301 row whose `url_from` is `/<current>`. HOLDS.
2. IF there is no such row, or its `url_to` does not match `^/[a-z0-9\-]+\Z` (one segment, no trailing newline), THEN THE function SHALL return `None`. HOLDS.
3. OTHERWISE THE function SHALL return `url_to` without its leading slash. HOLDS.
