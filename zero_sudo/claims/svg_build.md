---
anchor: zero_sudo:svg_build
code_hash: sha256:e023a98d8e7bfe84363e93fd03b9cc7a46ee3d2749349f466dd761733f175f4e
hash_algorithm: v2
---

# Claim: `_build(source, depth, budget, dropped_attributes=())` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns a new element built only from allowlisted names with validated attribute values (no transform and bounded size on svg), keeps the tail text of dropped children, returns None for a non-allowlisted tag and raises _Rejected past MAX_SVG_DEPTH. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_14_build_and_sanitize_element_rebuild_from_the_allowlist`.
