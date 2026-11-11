---
anchor: cloudflare:COMM_dns_problem_entry
code_hash: sha256:8cbc2e58740d4ca8a6d849fedfdf023d924d6d3180d1d14a1eb3adb84a737807
hash_algorithm: v2
review_tier: 1
---

# Claim: `_dns_problem_entry`

`_dns_problem_entry(plain, reason)` (static) builds the plan entry of action `problem` for one row, with its id, type, name, content, the reason, and the effective proxied flag; a problem entry is shown but never written.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.
