---
anchor: zero_sudo:svg_clean_attribute
code_hash: sha256:1d2a4022a4518414fa555fc14ae266fa622fa184d86539bf8f670acf1fb23a24
hash_algorithm: v2
---

# Claim: `_clean_attribute(name, value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns (canonical_name, cleaned_value) for an allowlisted attribute whose value passes its validator and the bad-value filter, and None for non-str input, a non-allowlisted name, a rejected value, or an aria attribute other than label, hidden and roledescription. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_11_clean_attribute_returns_canonical_name_and_value`.
