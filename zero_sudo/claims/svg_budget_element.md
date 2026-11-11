---
anchor: zero_sudo:svg_budget_element
code_hash: sha256:2f22d70277c3829e231312e9432b56f1e143ca0fec587212154845a2bc8d70aa
hash_algorithm: v2
---

# Claim: `_Budget.element(self)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): increments the element count and raises _Rejected on the call that makes it exceed MAX_SVG_ELEMENTS, never before. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_02_budget_element_counts_up_to_the_cap`.
