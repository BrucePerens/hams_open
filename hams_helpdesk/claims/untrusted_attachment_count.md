---
anchor: hams_helpdesk:untrusted_attachment_count
code_hash: sha256:de127ef3ebf1a2817bc9848f68b295fe86c1a836c62cc838a8af783d40983676
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin._untrusted_attachment_count` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE method SHALL require a single record, return the number of attachments counted as the helpdesk service account, never read an attachment's content, and return None (logging at info level) when the count raises. HOLDS.
