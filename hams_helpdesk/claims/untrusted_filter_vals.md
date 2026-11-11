---
anchor: hams_helpdesk:untrusted_filter_vals
code_hash: sha256:d864e0a6ebaab486e6b28bd488cf0c711562dad1ab399e463f42fc7d451ded6e
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin._untrusted_filter_vals` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL return a copy of `vals` (the argument is not modified) without the keys of `SUSPICION_FIELDS`; SHALL filter only fields named in `_untrusted_field_specs()` whose value is a non-empty string (html through `sanitize_html`, returned as Markup; text through `scan_text`, with newlines turned to spaces for `name` and `callsign`); SHALL list a `(field, Result)` for each of those and record in `raw` the original (cut to 100000 characters) of exactly the fields whose text changed. HOLDS.
