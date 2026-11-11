---
anchor: zero_sudo:svg_join_text
code_hash: sha256:a75eef07bf357252b5f9e09396ff6953622dbd7c123a6e33928d5722848ad77c
hash_algorithm: v2
---

# Claim: `_join_text(first, second, keeps_text)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns first+second, except that whitespace-only text where keeps_text is False becomes '\n'. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_12_clean_text_and_join_text_normalise_whitespace`.
