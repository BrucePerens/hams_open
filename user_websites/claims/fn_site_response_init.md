---
anchor: user_websites:fn_site_response_init
code_hash: sha256:65e02964de3a45bb81017771731794a72fa3a05d029a4d41130c6a2cd9ac4e57
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `SiteResponse.__init__` (`user_websites/controllers/owner_site.py`)

Read 2026-11-11 against the function's code and its callers.

1. WHEN constructed with `response`, THE function SHALL call `Exception.__init__` with the fixed message "owner site response" and store `response` unchanged on `self.response`. HOLDS.
2. THE function SHALL NOT validate, copy or wrap `response`, and SHALL NOT raise. HOLDS.
