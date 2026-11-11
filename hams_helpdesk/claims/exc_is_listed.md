---
anchor: hams_helpdesk:exc_is_listed
code_hash: sha256:898da9e039379bc252c89b1661d843ce945939af9259f4217264205df2b0f9b8
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ExceptionMember._is_listed`

THE method SHALL return True when a row exists for the fingerprint and `uid`, else True when the fingerprint already has at least `MEMBER_LIST_LIMIT` rows, else False. It writes nothing. HOLDS. Read 2026-11-11 against the code.
