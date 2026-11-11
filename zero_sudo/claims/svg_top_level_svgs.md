---
anchor: zero_sudo:svg_top_level_svgs
code_hash: sha256:0b1eea528ae6b621f7d3d7b066814a2df203c98b6c9718619f9354225dce802b
hash_algorithm: v2
---

# Claim: `_top_level_svgs(doc)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the svg elements of doc that have no svg ancestor, in document order. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
