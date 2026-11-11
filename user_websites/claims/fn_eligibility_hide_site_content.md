---
anchor: user_websites:fn_eligibility_hide_site_content
code_hash: sha256:ad23b6ad96b5bb067c7c933c23eb080ab63653e8478ac93c70a6ddaf6ed56341
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `ResUsersSiteEligibility._hide_site_content` (`user_websites/models/site_eligibility.py`)

Read 2026-11-11 against the function's code and its callers.

1. WHEN called, THE function SHALL find the user's published pages and published posts through `env_svc` and write `website_published`/`is_published` False and `hidden_by_eligibility` True on each. HOLDS.
2. THE function SHALL NOT unlink or archive anything and SHALL NOT touch an unpublished page or post, so content the owner unpublished is never marked. HOLDS.
