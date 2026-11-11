---
anchor: zero_sudo:svg_sanitize_xml_svgs
code_hash: sha256:9f9a36ffc2fbe82f744cf09622bef3ab1116d9c794c416ed26808942d23603ef
hash_algorithm: v2
---

# Claim: `sanitize_xml_svgs(root)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): replaces every outermost svg-named element under root by its allowlisted rebuild or removes it keeping its tail, and returns True exactly when a block looked hostile, was in a foreign namespace or the block count passed the cap. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_20_sanitize_xml_svgs_rebuilds_blocks_and_reports_hostility`.
