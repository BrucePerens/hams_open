---
anchor: zero_sudo:svg_in_blocked_context
code_hash: sha256:6b10b04be8c3570c7e83dc707faa53ffa617b2d44c64ce0e9340175a886a25ee
hash_algorithm: v2
---

# Claim: `_in_blocked_context(element)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns True when the element or an ancestor has a tag in _BLOCKED_CONTEXT, else False. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_16_a_block_in_a_blocked_context_is_removed_with_its_tails_kept`.
