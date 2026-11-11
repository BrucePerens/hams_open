---
anchor: zero_sudo:svg_clean_text
code_hash: sha256:b4ea13aa0c91988f279d65ee471932ba067d6fd458bfdc757265bc35e1572194
hash_algorithm: v2
---

# Claim: `_clean_text(text, keeps_text, budget)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns None for empty text, the control-character-free text when keeps_text, and otherwise '\n' for whitespace-only text and None for anything else. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_12_clean_text_and_join_text_normalise_whitespace`.
