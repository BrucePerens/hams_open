---
anchor: zero_sudo:svg_sanitize_element
code_hash: sha256:5be49f8df0e0f095737a3b6857612a591907f5e3a4ef7b2c6058c347aca4e4b4
hash_algorithm: v2
---

# Claim: `sanitize_svg_element(source, dropped_attributes=(), _totals=None)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns None for a non-svg element, when _totals.blocks passes MAX_TOTAL_SVG_BLOCKS, or when _build raises _Rejected; otherwise a rebuilt element carrying xmlns=SVG_NS. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_14_build_and_sanitize_element_rebuild_from_the_allowlist`.
