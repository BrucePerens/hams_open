---
anchor: zero_sudo:svg_small_margin
code_hash: sha256:f8be49092d7542f5c35824739879a2ecf45fdfc3d78448d394173ad5328890e2
hash_algorithm: v2
---

# Claim: `_small_margin(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the stripped value for a non-negative number up to 99 (px or unitless) or 9 (em, rem) and None for anything else. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_09_small_margin_and_svg_size_have_caps`.
