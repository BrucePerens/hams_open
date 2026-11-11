---
anchor: zero_sudo:svg_install
code_hash: sha256:bd21ce6c64ecd956214a86e4ce815a5b203bb66c75b2471f0368b4122b84adf0
hash_algorithm: v2
---

# Claim: `install()` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): replaces _Cleaner.__call__ with a wrapper marked _hams_svg_allowlist that runs clean_document, and does nothing when that marker is already on the current __call__. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_21_install_is_idempotent`.
