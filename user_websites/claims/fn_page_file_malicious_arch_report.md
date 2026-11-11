---
anchor: user_websites:fn_page_file_malicious_arch_report
code_hash: sha256:69489bd49be405ecc80d3dcaac0869318efa224b5b06e4c8e4f35ca4aa6b72bb
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `WebsitePage._file_malicious_arch_report` (`user_websites/models/website_page.py`)

Read 2026-11-11 against the function's code and its callers.

1. WHEN a content.violation.report exists for `target_url` and `reporter_id`, THE function SHALL call `action_take_action_and_strike()` on it (as `svc_uid`) and create nothing. HOLDS.
2. OTHERWISE THE function SHALL create an automated-alert report for the target and owner/group and call `action_take_action_and_strike()` on it. HOLDS.
