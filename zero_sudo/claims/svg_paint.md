---
anchor: zero_sudo:svg_paint
code_hash: sha256:881246050c10cf1e1044487fb9a8500aafe16d4b0a1d7ad4a8d767effdade8ca
hash_algorithm: v2
---

# Claim: `_paint(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the colour unchanged, rewrites `url( #id ) colour` to `url(#id) colour`, and returns None for a non-fragment url() or a value over 240 characters. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_05_paint_and_url_ref_keep_only_same_document_fragments`.
