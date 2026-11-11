---
anchor: hams_helpdesk:inbound_detect_spam_signals
code_hash: sha256:ba97e8840a237a3cf8f35fea8fe9fa742d61f112133127a1e37e53a73327a7ed
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `detect_inbound_spam_signals` (`models/inbound_spam_filter.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE function SHALL treat None subject or body as empty and never raise on a malformed link; SHALL add one reason per `SPAM_SUBJECT_PATTERNS` match in the subject; SHALL add the housing-solicitation reason only when the tag-stripped body matches both the salutation and the offer pattern; and SHALL add a brand-impersonation reason for a `KNOWN_BRAND_DOMAINS` brand only when the message has at least one link, names the brand as a whole word, and at least one link domain is not the brand's own domain (the sender address is never consulted). HOLDS.
