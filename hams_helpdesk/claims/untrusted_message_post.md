---
anchor: hams_helpdesk:untrusted_message_post
code_hash: sha256:752b63e6e8ce77e7b6b36d07c285bdbc0cf0b21f49d558bcfa147293ed0fb006
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `HamsHelpdeskUntrustedMixin.message_post` (`models/untrusted_mixin.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE method SHALL, unless the context has `untrusted_filter_bypass`, replace a present body with its `sanitize_any` text (limit 50000), replace `subject`, `email_from` and `reply_to` with their `safe_header` text (limit 300) and each attachment name with its `safe_filename` text; SHALL pop the findings parked for `message_id` by `message_parse` and merge them (findings capped at `MAX_FINDINGS`, larger score, removed summed); and SHALL record the result with source `message` after the parent post. HOLDS.
