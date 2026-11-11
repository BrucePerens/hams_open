---
anchor: user_websites:fn_page_remove_keeping_tail
code_hash: sha256:a21a928fe64d4a59b5f555335b62fcd89d09a01a7234642417c120698233ef20
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `_remove_keeping_tail` (`user_websites/models/website_page.py`)

Read 2026-11-11 against the function's code and its callers.

1. IF `element` has no parent, THEN THE function SHALL do nothing. HOLDS.
2. IF `element.tail` is non-empty, THEN THE function SHALL append it to the previous sibling's tail, or to the parent's text when there is no previous sibling, before removal. HOLDS.
3. THE function SHALL remove `element` and its descendants from the parent. HOLDS.
