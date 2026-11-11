---
anchor: zero_sudo:css_clean_rules
code_hash: sha256:267b32fa650162863fc6573a6bcb06d5ec78d5fa239e445bba720d24d3adc5e2
hash_algorithm: v2
---

# Claim: `_clean_rules(text, depth, counter, in_keyframes=False)` (`zero_sudo/css_sanitizer.py`)

Behaviour (falsifiable): returns only rules with an allowed selector, `@media` with a plain condition (to depth 2) and top-level `@keyframes`, each rebuilt from _clean_declarations output, drops every other at-rule and any rule past 2000, and reports dropped True when it removed anything. HOLDS. Read 2026-11-11 against the source. Test: `TestCssSanitizerFunctions.test_06_clean_rules_drops_what_is_not_allowed_and_keeps_the_rest`.
