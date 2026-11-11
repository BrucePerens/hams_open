---
anchor: hams_helpdesk:untrusted_write
code_hash: sha256:368627b0f127f1997e3b886b480319be38c869046127891fd21d90781ec5e64e
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin.write` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL call the parent write unchanged when the context has `untrusted_filter_bypass` or when `vals` names neither a filtered field nor a suspicion field; otherwise it SHALL write the filtered values (suspicion keys removed) and, when anything was filtered, record the findings on each ticket with source `write`. HOLDS.
