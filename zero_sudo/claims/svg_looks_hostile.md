---
anchor: zero_sudo:svg_looks_hostile
code_hash: sha256:3ba685d6dbe609e9dc52080de7766c3a075f6ccb8bae93bf9af6956a149881ef
hash_algorithm: v2
---

# Claim: `_looks_hostile(source)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns True when the block has a hostile element name, an on* attribute, or a URL-like attribute with a javascript, vbscript or data scheme, and False for metadata, unknown attributes and a plain 'Data:' in a label. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_18_looks_hostile_tells_attacks_from_editor_leftovers`.
