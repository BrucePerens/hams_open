---
anchor: zero_sudo:css_scan
code_hash: sha256:70d1faa517543e1cc1521d22182cce4f3c96a451c678e2d9d531116f04ff64ea
hash_algorithm: v2
---

# Claim: `_scan(text, start, stops)` (`zero_sudo/css_sanitizer.py`)

Behaviour (falsifiable): returns the index of the first character of `stops` at or after `start` that is outside quotes and outside parentheses, else len(text); parenthesis depth never goes below 0. HOLDS. Read 2026-11-11 against the source. Test: `TestCssSanitizerFunctions.test_01_scan_stops_only_outside_strings_and_parentheses`.
