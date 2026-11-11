---
anchor: hams_helpdesk:untrusted_leading_number
code_hash: sha256:7c976eb65ffee38ffd3cbd17ab7f291b664b062399b574450a107622c8c7b7d7
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `_leading_number` (`models/untrusted_text.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE function SHALL return the float of the leading `-?[\d.]+` of the value, None when there is none or the float conversion fails (`1.2.3`, `.`), and `inf` for a very long digit run. HOLDS.
