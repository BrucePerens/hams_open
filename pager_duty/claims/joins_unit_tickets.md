---
anchor: pager_duty:joins_unit_tickets
code_hash: sha256:10a88c69738b0d8fb32c93e0f40c0127a84e505550ba218c1626fe2fb8480254
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: incident.py `PagerIncident._joins_unit_tickets` (`pager_duty/models/incident.py`)

Read 2026-11-11, whole function. Tier 2: Reads one system parameter.

1. THE method SHALL read the system parameter `pager_duty.helpdesk_model` through the zero_sudo helper, defaulting to `hams_helpdesk.ticket` when unset.
2. THE method SHALL return True exactly when that value equals `hams_helpdesk.ticket`, and False for any other model name. HOLDS.
