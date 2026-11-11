---
anchor: hams_helpdesk:inbound_registrable_domain
code_hash: sha256:3dbf86cabc4cd50c0b80f6c9d7865fe43c2e403001e20cf4a59f5c8587709ac8
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `_registrable_domain` (`models/inbound_spam_filter.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- WHEN given a falsy hostname THE function SHALL return the empty string; otherwise it SHALL return the hostname stripped of surrounding whitespace, lower-cased, with exactly one leading `www.` removed and nothing else changed (no public-suffix logic, so `accounts.intuit.com` is returned whole). HOLDS.
