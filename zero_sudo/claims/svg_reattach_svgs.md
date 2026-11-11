---
anchor: zero_sudo:svg_reattach_svgs
code_hash: sha256:742e8e9ca2e8cc4a29e948e2f721844f9fd70e3ed91e37623233639c29f8ec9a
hash_algorithm: v2
---

# Claim: `_reattach_svgs(doc, stash, nonce)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): replaces each placeholder comment of this nonce with its stashed svg unless the stash entry is None or the parent is in a blocked context, and removes the placeholder in every case keeping its tail text. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
