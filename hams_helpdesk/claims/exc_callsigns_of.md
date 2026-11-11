---
anchor: hams_helpdesk:exc_callsigns_of
code_hash: sha256:63f92a8193825825521085cb9272d5fe8f0d0ac3988a1ddb7c1251a2c4e1881b
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ExceptionMember._callsigns_of`

THE method SHALL return a dict with one key per user in `users`, each a list. The list holds the user's own `callsign` stripped and upper-cased when set, then each `ham.callbook` row linked to that user (searched through the callbook service environment, at most 20 per user) whose callsign, stripped and upper-cased, is non-empty and not already in that user's list. A callbook row for a user not in `users` is never read. HOLDS. Read 2026-11-11 against the code.
