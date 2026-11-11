---
anchor: zero_sudo:svg_check_regex
code_hash: sha256:b8db3d72dca85fc9dd76ee6401d2ade1c2653ace182d2104175eb4f11f39da6e
hash_algorithm: v2
---

# Claim: `_check_regex(regex, limit=SCALAR_LIMIT)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns a function that returns the stripped value when its length is within limit and regex.fullmatch accepts it, else None. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_04_check_enum_and_check_regex_build_validators`.
