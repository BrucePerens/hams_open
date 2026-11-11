---
anchor: zero_sudo:css_sanitize_stylesheet
code_hash: sha256:2fe9453fbd9af9020e1cb929c4e251c4d0a1c259a417ee447047f9cebb94eb14
hash_algorithm: v2
---

# Claim: `sanitize_stylesheet(text)` (`zero_sudo/css_sanitizer.py`)

Behaviour (falsifiable): returns ('', False) for blank text, ('', True) for text over 64 KiB or with a control character, and otherwise the _clean_rules rewrite of the comment-free text with dropped True when anything but a comment was removed or a comment was unterminated. HOLDS. Read 2026-11-11 against the source. Test: `TestCssSanitizerFunctions.test_07_sanitize_stylesheet_rewrites_the_sheet`.
