---
anchor: hams_helpdesk:inbound_ses_verdict_reasons
code_hash: sha256:406f2335363d122343dba1ec53a4ba31900a4340c250aa964fa599e252a17142
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ses_verdict_reasons` (`models/inbound_spam_filter.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE function SHALL return the reason `Amazon SES spam scan verdict: FAIL` when `verdicts['spam']` is exactly `FAIL`, the virus reason likewise, both in that order, and an empty list for None, an empty dict, PASS, GRAY, PROCESSING_FAILED or any other value. HOLDS.
