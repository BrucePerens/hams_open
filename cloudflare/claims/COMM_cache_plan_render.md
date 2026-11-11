---
anchor: cloudflare:COMM_cache_plan_render
code_hash: sha256:498c494b5b895673be3b2e78ef72498c8dc575597a895055ec0481f64b89921e
hash_algorithm: v2
review_tier: 1
---

# Claim: `render`

`render(plan, ruleset_state)` returns the human text of a cache plan from `merge`: a header with `ruleset_state`, one line per entry (CREATE, UPDATE, DELETE, KEEP, UNCHANGED, in that order, then by name) with its reason, the resulting rule order, and a summary of how many managed rules change; unmanaged rules are never altered. It is pure.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.
