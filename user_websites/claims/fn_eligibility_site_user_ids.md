---
anchor: user_websites:fn_eligibility_site_user_ids
code_hash: sha256:6eb3ed48dfd2d541bb7b155c6f3e559870f5ce0060fa2bf8a086500259981538
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ResUsersSiteEligibility._website_site_user_ids` (`user_websites/models/site_eligibility.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL return a list of the ids of every active res.users row whose `website_slug` is non-null, non-empty, and whose `is_service_account` is not true. HOLDS (one static SQL query).
