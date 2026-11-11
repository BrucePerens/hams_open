---
anchor: hams_helpdesk:untrusted_create
code_hash: sha256:c340b37d33b83eb9ea9535dce9da3b4ba0a2666698fa047f7e0b6cc753c8c91d
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin.create` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL filter every values dict before the parent create, store `suspicion_score`, `suspicion_removed` and (at or above the threshold) `suspicious` from the summary, discarding any value the caller sent for them, and SHALL then write the evidence log for each created record with source `create`. HOLDS.
