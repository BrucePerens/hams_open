---
anchor: hams_helpdesk:untrusted_safe_view
code_hash: sha256:f30a774af1c09bb5922f47c8517fe4d6b869578ca58fa206c06e81e52992a260
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin.safe_view` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL return one dict per ticket built only from filtered text (the subject, description and callsign re-filtered on every call), with the findings carrying only `vector`, `location` and `count` (no excerpt), `suspicious` true when the stored flag is set or the fresh score reaches the threshold, the recorded intake score included as a finding, the body cut to `max_chars` when that is non-zero, and the text wrapped by `wrap_untrusted`. HOLDS.
