---
anchor: zero_sudo:svg_check_enum
code_hash: sha256:cd1eb4cea823b99bfc1d643d95890c7c6218931c74f8209a9529e9c20b93ca4d
hash_algorithm: v2
---

# Claim: `_check_enum(name)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns a function that returns its argument when it is in _ENUMS[name] and None otherwise. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_04_check_enum_and_check_regex_build_validators`.
