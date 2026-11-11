---
anchor: user_websites:fn_page_malicious_arch_violation_targets
code_hash: sha256:41aac5fef851e0828007177e5fbb3b6b5486cd1eaaaf3efff9bdbdec1a396788
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `WebsitePage._malicious_arch_violation_targets` (`user_websites/models/website_page.py`)

Read 2026-11-11 against the function's code and its callers.

1. IF `vals` name an owner or group, OR there are no records, THEN THE function SHALL return exactly one `(owner_id, group_id, url)` with the URL from `vals`, else the first record's (None when none). HOLDS.
2. OTHERWISE THE function SHALL return one tuple per distinct (owner, group) among `records`, with the URL from `vals` or that group's first record. HOLDS.
