---
anchor: zero_sudo:svg_call_totals_init
code_hash: sha256:fbb96129ac3883aa3f02110a5e4c8df064e10bcec6dc3a41eb3ca15804ec524c
hash_algorithm: v2
---

# Claim: `_CallTotals.__init__(self)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): creates an object whose `blocks` and `size` start at 0. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_01_call_totals_and_budget_start_at_zero`.
