---
anchor: zero_sudo:svg_viewbox
code_hash: sha256:ce698c6113dab49e3be5dda859a875509b4e0fc96715d5ad78aefb9bcd5fc756
hash_algorithm: v2
---

# Claim: `_viewbox(value)` (`zero_sudo/svg_sanitizer.py`)

Behaviour (falsifiable): returns the four numbers joined by single spaces, and None unless there are exactly four numeric parts. HOLDS. Read 2026-11-11 against the source. Test: `TestSvgSanitizerFunctions.test_06_viewbox_needs_four_numbers`.
