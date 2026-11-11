---
anchor: zero_sudo:css_clean_value
code_hash: sha256:64964e98a5a00f6089f8d98753171ea89c1205c88ca0aa946e370b7e7a9741c5
hash_algorithm: v2
---

# Claim: `_clean_value(prop, raw)` (`zero_sudo/css_sanitizer.py`)

Behaviour (falsifiable): returns None for an empty, over-long, forbidden-pattern, unbalanced, non-fragment url() or fixed/sticky position value, and otherwise the whitespace-collapsed value with ` !important` preserved. HOLDS. Read 2026-11-11 against the source. Test: `TestCssSanitizerFunctions.test_04_clean_value_refuses_what_fetches_or_runs`.
