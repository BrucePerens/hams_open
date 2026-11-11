---
anchor: zero_sudo:css_parse_blocks
code_hash: sha256:06837ce5704dcd4841f4e226ecbbfd0ca50f5d97dae25143a1b70f5760ebb7cc
hash_algorithm: v2
---

# Claim: `_parse_blocks(text)` (`zero_sudo/css_sanitizer.py`)

Behaviour (falsifiable): returns one (prelude, body) item per block or `;`-terminated statement in order; body is the text inside the matching braces or None for a statement, an unclosed block or a trailing fragment. HOLDS. Read 2026-11-11 against the source. Test: `TestCssSanitizerFunctions.test_03_parse_blocks_splits_rules_and_statements`.
