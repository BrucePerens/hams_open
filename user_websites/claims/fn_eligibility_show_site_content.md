---
anchor: user_websites:fn_eligibility_show_site_content
code_hash: sha256:5014335062f5d3e7c013340140fe5496e2b758e074699ae19c39d8adfd0a8370
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `ResUsersSiteEligibility._show_site_content` (`user_websites/models/site_eligibility.py`)

Read 2026-11-11 against the function's code and its callers.

1. IF the user `is_suspended_from_websites`, THEN THE function SHALL do nothing. HOLDS (early return).
2. OTHERWISE THE function SHALL republish exactly the user's pages and posts marked `hidden_by_eligibility` and clear the mark. HOLDS.
3. THE function SHALL NOT republish anything not marked. HOLDS.
