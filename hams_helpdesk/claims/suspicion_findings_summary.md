---
anchor: hams_helpdesk:suspicion_findings_summary
code_hash: sha256:6b9c0f89809231c569701801f1f303d4510ed0825b0f1822d30c011757d92e7e
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskTicketSuspicion._compute_findings_summary` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE method SHALL set `findings_summary` to the `vector xcount in location` entries of `findings_json` joined by `; ` and cut to 500 characters, with no excerpt text. HOLDS.
