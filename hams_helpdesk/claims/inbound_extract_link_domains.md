---
anchor: hams_helpdesk:inbound_extract_link_domains
code_hash: sha256:06f7b7de177d72725211484492cb0e47c421b218da7696e1fe7d283d0eca94b8
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `_extract_link_domains` (`models/inbound_spam_filter.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE function SHALL return the set of lower-cased hostnames of every `href="..."` or `href='...'` value in `body_html` (None or empty gives the empty set), SHALL skip a link whose host is missing (relative link) and SHALL skip a link for which `urlsplit` raises ValueError instead of raising. HOLDS.
