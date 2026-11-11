---
anchor: user_websites:fn_site_slug_like_escape
code_hash: sha256:a80ce93940d9cdd90c65568fcb586bfff5684e1fc9d29e1ceecbfc41e681e233
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `_like_escape` (`user_websites/models/site_slug.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL return `text` with each backslash, percent sign and underscore preceded by one backslash (backslash escaped first so the others are not doubled), and every other character unchanged. HOLDS.
