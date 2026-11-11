---
anchor: zero_sudo:svg_size
code_hash: sha256:b4856c0723f85eee92eb5ca4b10ceb58205f4947c40f12d95faf772ab14fc0e0
hash_algorithm: v2
---

# Claim: `_svg_size(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns 'auto' or the stripped number-with-unit when within 4000 (px, none), 100 (%) or 250 (em, rem), and None otherwise. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_09_small_margin_and_svg_size_have_caps`.
