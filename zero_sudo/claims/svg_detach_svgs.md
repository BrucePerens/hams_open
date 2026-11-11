---
anchor: zero_sudo:svg_detach_svgs
code_hash: sha256:0945db59f73382cc483c9c219c997d444ac4abe09dc2c42840ca97a869a411e4
hash_algorithm: v2
---

# Claim: `_detach_svgs(doc, nonce, dropped_attributes)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): replaces every top-level svg of doc by a comment 'hamssvg<nonce>x<index>x' and returns {index: sanitize_svg_element result}, turning a root svg into a div. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
