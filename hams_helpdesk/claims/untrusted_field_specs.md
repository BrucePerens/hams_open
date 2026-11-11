---
anchor: hams_helpdesk:untrusted_field_specs
code_hash: sha256:0d61a4b8adbf15d6e720bbbacf248593a2cfe7a541328fa5e5cda42f8610c573
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin._untrusted_field_specs` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE method SHALL return a new dict copied from `UNTRUSTED_FIELDS` on every call, so an override that adds a field never alters the module-level table. HOLDS.
