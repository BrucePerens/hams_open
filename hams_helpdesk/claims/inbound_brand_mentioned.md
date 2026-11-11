---
anchor: hams_helpdesk:inbound_brand_mentioned
code_hash: sha256:285ee18ad7f68df85da7a071026e771e8bd50a46b6d485e2ec75e069a50519e8
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `_brand_mentioned` (`models/inbound_spam_filter.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE function SHALL return True exactly when `brand` occurs in `text` as a whole word (word boundaries on both sides), ignoring case; the brand is regex-escaped, so it is never read as a pattern. HOLDS.
