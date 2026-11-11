---
anchor: zero_sudo:css_clean_declarations
code_hash: sha256:bd9b9deb49d8fcde7b0c19a5145711a8e30ca90af9a51ee89703f5bfbfcc69d9
hash_algorithm: v2
---

# Claim: `_clean_declarations(body)` (`zero_sudo/css_sanitizer.py`)

Behaviour (falsifiable): returns the declarations whose property is a plain lower-cased identifier outside the bad list and whose value passes _clean_value, at most 200, as `prop:value;...`, and dropped True exactly when one was removed. HOLDS. Read 2026-11-11 against the source. Test: `TestCssSanitizerFunctions.test_05_clean_declarations_keeps_the_good_ones`.
