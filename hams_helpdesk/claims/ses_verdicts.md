---
anchor: hams_helpdesk:ses_verdicts
code_hash: sha256:8191f897c204f46661d0cd19f2d09c88eba0ccb6af6dc33f52c1cbfbec40fc3d
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ses_verdicts` (`models/inbound_spam_filter.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE function SHALL return a dict holding `spam` and `virus` only for those of `X-SES-Spam-Verdict` and `X-SES-Virus-Verdict` that are present and non-empty, each value stripped, upper-cased and cut to 32 characters; it SHALL NOT interpret the values. HOLDS.
