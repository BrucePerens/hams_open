---
anchor: user_websites:fn_users_self_writeable_fields
code_hash: sha256:595fa3daf5870aff4d879b121887e1e80552c032d6431098b7490d251055d363
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `ResUsers.SELF_WRITEABLE_FIELDS` (`user_websites/models/res_users.py`)

Read 2026-11-11 against the function's code and its callers.

1. THE property SHALL return the inherited `SELF_WRITEABLE_FIELDS` list followed by exactly `privacy_show_in_directory` and `website_slug`. HOLDS.
2. THE property SHALL NOT include `website_ineligible` or any suspension field, so a member cannot lift their own eligibility hiding. HOLDS (the list has only those two additions).
