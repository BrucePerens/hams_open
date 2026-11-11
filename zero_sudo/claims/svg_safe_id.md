---
anchor: zero_sudo:svg_safe_id
code_hash: sha256:57840b0a4206a2995df699f51180d880f8d2ca9abbb2e0951ac9ac4dd53c957d
hash_algorithm: v2
---

# Claim: `_safe_id(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the stripped id when it matches the identifier pattern, does not start with `__` and is not (case-insensitively) a clobbering name, else None. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_07_safe_id_refuses_clobbering_names`.
