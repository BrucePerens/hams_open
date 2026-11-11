---
anchor: hams_helpdesk:inbound_stash_for
code_hash: sha256:d1ef09a252affac50563a54f8316dfa0de1ff5f6416b140c3467a2466c1e72fc
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `stash_for` (`models/mail_thread_untrusted.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 2.

- THE function SHALL return the dict stored under `MAIL_STASH_KEY` in `env.cr.cache`, creating an empty one on first use, so that every call on one cursor returns the same dict; it SHALL NOT persist anything (the cursor cache is cleared on commit and rollback). HOLDS.
