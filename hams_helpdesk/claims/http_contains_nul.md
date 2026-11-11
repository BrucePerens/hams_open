---
anchor: hams_helpdesk:http_contains_nul
code_hash: sha256:324398dc72c2c698071a3071d7b3d7fc2169467685d4b287219ac2bcff8d353c
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ir_http_exception._contains_nul`

THE function SHALL return True when any str or bytes found in `value` contains a NUL, visiting dict keys and values and the items of lists, tuples, sets and frozensets to any depth (iteratively), and False otherwise. Other types are ignored. It does not modify `value`. HOLDS. Read 2026-11-11 against the code.
