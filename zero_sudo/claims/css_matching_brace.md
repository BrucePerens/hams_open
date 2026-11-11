---
anchor: zero_sudo:css_matching_brace
code_hash: sha256:db9e114075d037547ef7931ae800ed5c821c25e890fc9742f411fe2c2f2fc288
hash_algorithm: v2
---

# Claim: `_matching_brace(text, start)` (`zero_sudo/css_sanitizer.py`)

Behaviour (falsifiable): returns the index of the `}` that brings brace depth from 1 back to 0, skipping braces inside quotes, or None when depth never returns to 0. HOLDS. Read 2026-11-11 against the source. Test: `TestCssSanitizerFunctions.test_02_matching_brace_skips_nested_braces_and_strings`.
