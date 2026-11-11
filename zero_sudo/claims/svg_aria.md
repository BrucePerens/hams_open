---
anchor: zero_sudo:svg_aria
code_hash: sha256:1fbb2aa86c6082e6d91cedc7a0820002499e23909fb82f8369f50ee06c19e5ef
hash_algorithm: v2
---

# Claim: `_aria(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the value unchanged when it matches the 200-character no-markup pattern and None otherwise. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_08_aria_value_refuses_markup_characters`.
