---
anchor: pager_duty:note_unit_tickets
code_hash: sha256:128a1dfe3e86a22111175eab3f5b6cf3d8c9bb82d25f94f4add25b7078f24fe4
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: incident.py `PagerIncident._note_unit_tickets` (`pager_duty/models/incident.py`)

Read 2026-11-11, whole function. Tier 2: Posts a chatter note as the internal mail service user.

1. THE method SHALL post one internal note (`mail.mt_note`) on `incident`, as the `zero_sudo.mail_service_internal` user, with no recipients.
2. THE note text SHALL be `Tracked on the per-unit ticket(s) <listing>; no separate ticket was opened.` where the listing is `#<ticket> (<unit>)` for each item of `unit_tickets`, sorted by unit name, joined by `, `.
3. THE method returns nothing and changes nothing else. HOLDS.
