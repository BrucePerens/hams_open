---
anchor: user_websites:fn_owner_back
code_hash: sha256:536845980754c2f83d750363faf12162da7a75860b5d64bb972ad9bb5a6ec8c5
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `UserWebsitesOwnerController._back` (`user_websites/controllers/owner_site.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL return `request.redirect` to `/my/site/<site_slug>?<flag>` with code 303, `site_slug` percent-encoded with no safe characters (so a `/` cannot add a path segment) and `flag` inserted as given. HOLDS.
2. THE function SHALL NOT read the database or write anything. HOLDS.
