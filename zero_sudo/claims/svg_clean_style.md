---
anchor: zero_sudo:svg_clean_style
code_hash: sha256:ca010448a0284e261fac0ca27e90665b6dd5151bca930f3c3fbfc38a3bb624cc
hash_algorithm: v2
---

# Claim: `_clean_style(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the declarations whose property is in _STYLE_PROPS and whose value passes its validator joined by ';', and None when there are none or the value has /*, @, a backslash or is over 2000 characters. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_10_clean_style_keeps_allowlisted_declarations`.
