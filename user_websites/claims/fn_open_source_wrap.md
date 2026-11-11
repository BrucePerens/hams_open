---
anchor: user_websites:fn_open_source_wrap
code_hash: sha256:1286ac92f53c02d389e5151adaf0ae74271adb0965938efaa2d2ca41bed176e7
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `_wrap` (`user_websites/tests/open_source_defaults.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL replace `cls.setUpClass` with one that commits the open-source slug policy, removes the form-shield tuning keys, runs the original `setUpClass`, then patches `res.users._website_eligibility_gaps` to the open-source version; each is undone by a class cleanup. HOLDS.
