---
anchor: hams_helpdesk:untrusted_summary
code_hash: sha256:977197841f47766e2d6793d3e58d8947c664c3b176245661ef31dc5e2505426c
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin._untrusted_summary` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL return the merged findings and removed count of the given results, extended with the `scan_concatenation` findings of the fields' plain text plus `extra_parts`, and a score equal to the merged score plus the weight of each `split_` finding, never above 100. HOLDS.
