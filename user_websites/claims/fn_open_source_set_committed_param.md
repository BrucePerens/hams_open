---
anchor: user_websites:fn_open_source_set_committed_param
code_hash: sha256:23362751e86ac7d360b02bf4d489760d3b704cb701ade3661b2d244fa5a17384
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `_set_committed_param` (`user_websites/tests/open_source_defaults.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE function SHALL, on its own committed cursor, read the key's old value, then delete the row when `value` is None, else upsert it, and return the old value or None. HOLDS.
