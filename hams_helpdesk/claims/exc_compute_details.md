---
anchor: hams_helpdesk:exc_compute_details
code_hash: sha256:be3a4bc438abe784e8e1997b36e7cf15a87d33633b50d6488115925ef10f24cf
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ExceptionMember._compute_details`

THE method SHALL leave member name, callsigns, contact name, email and label False and `contact_kind` `closed` for a row with no user (label "The account no longer exists") or an archived user (label "The account is closed..."); callsigns are looked up only for active users. For an active user it sets `member_name` to the user's name, `callsigns` to the comma-joined list (False when empty), and `contact_kind` and the contact from `_contact_user`: a guardian or withheld kind gets its own label, any other kind the label "The member". `contact_name` and `contact_email` are set only when the contact is active; the email is the contact's trimmed email, else its login when the login contains `@`, else False. HOLDS. Read 2026-11-11 against the code.
