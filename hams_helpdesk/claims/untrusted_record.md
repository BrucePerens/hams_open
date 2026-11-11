---
anchor: hams_helpdesk:untrusted_record
code_hash: sha256:23aeb62982ca16fd8ba398eb659ab86f174f1ebf952f7b0cd9b91b7f59e9a829
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin._untrusted_record` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL do nothing for an empty recordset or when findings, removed and raw are all empty; otherwise, for each ticket, it SHALL create one suspicion log as the helpdesk service account (no sudo), add `removed` to `suspicion_removed`, raise `suspicion_score` to the maximum of old and new, set `suspicious` when that reaches `SUSPICION_THRESHOLD`, and write those counters with `untrusted_filter_bypass` so the filter does not re-run. HOLDS.
