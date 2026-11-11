---
anchor: user_websites:fn_open_source_pin_package
code_hash: sha256:41d50e25db60064900dd98402d6bd02b897213914a1fa8cd7d8a394594666bb9
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `pin_package` (`user_websites/tests/open_source_defaults.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL wrap once each TransactionCase/HttpCase class defined in a loaded module named `<package>.test_*` and mark it `_open_source_pinned`. HOLDS.
