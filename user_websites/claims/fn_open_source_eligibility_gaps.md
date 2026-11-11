---
anchor: user_websites:fn_open_source_eligibility_gaps
code_hash: sha256:04f1b4bee9db931f20946443a2088f1728ecce38c1acc71f70be024e3a3dd891
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `_open_source_eligibility_gaps` (`user_websites/tests/open_source_defaults.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL return the result of `ResUsersSiteEligibility._website_eligibility_gaps(self)`, bypassing any override. HOLDS.
