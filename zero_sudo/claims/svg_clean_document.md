---
anchor: zero_sudo:svg_clean_document
code_hash: sha256:75c1288b7027e6540f903fcedbbc2401ecc7022cf293ce655e98d1e14c47b059
hash_algorithm: v2
---

# Claim: `clean_document(doc, run_cleaner, dropped_attributes=())` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns run_cleaner(doc)'s result, calling it on the document with every top-level svg replaced by a placeholder and restoring the rebuilt blocks afterwards; with no svg it only calls run_cleaner. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
