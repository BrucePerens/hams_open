---
anchor: zero_sudo:svg_url_ref
code_hash: sha256:4e4c6872f8c8eb73fe4d19220d2b54f71ce7536a0e1b75853a00df398985a894
hash_algorithm: v2
---

# Claim: `_url_ref(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns 'none' for none, 'url(#id)' for a same-document fragment reference and None for anything else. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_05_paint_and_url_ref_keep_only_same_document_fragments`.
