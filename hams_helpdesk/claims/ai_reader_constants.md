---
anchor: hams_helpdesk:ai_reader_constants
code_hash: sha256:3b7b2cb0d929a1a79efa9726b61b5a8a191d4a886374c56ce3ce09e8ed654b1a
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `ai_reader_rule._constants`

THE function SHALL return `(consts, value)` for a parsed module. `consts` holds each module-level single-name assignment whose right side resolves to a literal: a constant, a list or tuple of resolvable items (a tuple becomes a list), a name already in `consts`, or the sum of two such lists. An assignment that does not resolve (a call, an unknown name, a list holding an unresolvable item, a tuple target) is absent from `consts`, not an error. `value(node)` resolves any expression the same way and returns None when it cannot. HOLDS. Read 2026-11-11 against the code.
