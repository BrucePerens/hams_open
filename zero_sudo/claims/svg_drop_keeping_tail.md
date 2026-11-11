---
anchor: zero_sudo:svg_drop_keeping_tail
code_hash: sha256:dd58f4302665ccc63fe7edb3a3f1ec0a422d979694e565f072ae244132aff1a5
hash_algorithm: v2
---

# Claim: `_drop_keeping_tail(element)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): removes the element from its parent and appends its tail to the previous sibling's tail or the parent's text; does nothing when it has no parent. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_19_drop_keeping_tail_hands_the_tail_to_the_neighbour`.
