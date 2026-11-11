---
anchor: hams_helpdesk:inbound_domain_matches_brand
code_hash: sha256:a4a2437235434b5b892a34a225c16037e79c6add38de29f838bbc4ce5f9d222d
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `_domain_matches_brand` (`models/inbound_spam_filter.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE function SHALL return False for an empty hostname, and otherwise True exactly when the `_registrable_domain` of the hostname equals one of `real_domains` or ends with `"." + that domain`; so `notintuit.com` and `intuit.com.evil.example` do not match `intuit.com`. HOLDS.
