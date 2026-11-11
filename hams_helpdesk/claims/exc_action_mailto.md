---
anchor: hams_helpdesk:exc_action_mailto
code_hash: sha256:5addce8880126b727ecb20e49fa3f0f189977003972d03f640acae431880f7b2
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ExceptionMember.action_mailto`

THE method SHALL require a single row, call `_contact_partner()` first (so it raises the same `UserError` when there is nobody to write to), and return an `ir.actions.act_url` action with `target` `self` and the URL `mailto:<contact_email quoted keeping @>?subject=<REPLY_SUBJECT quoted>`. It sends nothing and puts no body text in the link. HOLDS. Read 2026-11-11 against the code.
