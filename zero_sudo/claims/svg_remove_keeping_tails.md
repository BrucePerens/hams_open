---
anchor: zero_sudo:svg_remove_keeping_tails
code_hash: sha256:7a7bff7860b6d319b8ac3796cc986087cd87cf71e707d8ee41c445a88f8f728f
hash_algorithm: v2
---

# Claim: `_remove_keeping_tails(parent, comments)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): removes each listed comment child of parent in one pass and leaves its tail text joined to the previous text or tail. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_17_remove_keeping_tails_and_store_text_merge_tail_text`.
