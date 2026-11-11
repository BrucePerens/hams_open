---
anchor: zero_sudo:svg_local_name
code_hash: sha256:6152a756ba445cefdb6e088a30a57a34c9341e517800edc683439515542a00ca
hash_algorithm: v2
---

# Claim: `svg_local_name(tag)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the lower-cased local name for a bare tag or a tag in the SVG namespace, and None for a non-str, a prefixed name or any other namespace. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_13_svg_local_name_accepts_bare_and_svg_namespace_names`.
