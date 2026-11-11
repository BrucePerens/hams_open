---
anchor: zero_sudo:svg_store_text
code_hash: sha256:9037357ed405954bddd175a75951ef979b179e540037f17bccf2b2b3d5cfcb31
hash_algorithm: v2
---

# Claim: `_store_text(parent, holder, pieces)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): sets parent.text (holder None) or holder.tail to ''.join(pieces), or None when that is empty. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_17_remove_keeping_tails_and_store_text_merge_tail_text`.
