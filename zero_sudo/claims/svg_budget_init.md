---
anchor: zero_sudo:svg_budget_init
code_hash: sha256:2c6cc8cd7e6b04ded00ad8acc5bee99803967d8b26e2c7f110ef710b0fc3d9df
hash_algorithm: v2
---

# Claim: `_Budget.__init__(self, totals=None)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): creates a budget with elements 0, size 0 and `totals` set to the argument (None when omitted). HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_01_call_totals_and_budget_start_at_zero`.
