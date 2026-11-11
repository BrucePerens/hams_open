---
anchor: hams_helpdesk:untrusted_log_only
code_hash: sha256:149d941a8ed4b4b4f01197c065f96691a8613afef348be0c7a438d65052a7c3c
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin._untrusted_log_only` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL write nothing when findings, removed and raw are all empty, and otherwise exactly one suspicion log for the single ticket, as the helpdesk service account, without touching the ticket's counters (create() stored them). It calls `self.id`, so it is for a single record. HOLDS.
