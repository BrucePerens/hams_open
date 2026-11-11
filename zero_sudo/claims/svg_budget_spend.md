---
anchor: zero_sudo:svg_budget_spend
code_hash: sha256:4628b37a46093208c1acd7fef29e98cfa4321a4ce9ab80e761081f7ceb14fc84
hash_algorithm: v2
---

# Claim: `_Budget.spend(self, amount)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): adds amount to the block size and to totals.size when totals is set, and raises _Rejected when the block exceeds MAX_SVG_BYTES or the totals exceed MAX_TOTAL_SVG_BYTES. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_03_budget_spend_caps_the_block_and_the_call`.
