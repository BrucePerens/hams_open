---
anchor: hams_helpdesk:exc_contact_partner
code_hash: sha256:9f676397e8ef98e899eabd4dfca73043b2dd3f8d658ce4a2efc5227f44d4a278
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ExceptionMember._contact_partner`

THE method SHALL require a single row, and raise `UserError` ("There is nobody to write to for this member.") when the user is archived, the contact kind is `closed` or `withheld`, the contact is empty or inactive, or the row has no `contact_email`. Otherwise it returns the contact's `partner_id`. HOLDS. Read 2026-11-11 against the code.
