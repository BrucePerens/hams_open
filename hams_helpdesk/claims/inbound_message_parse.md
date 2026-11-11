---
anchor: hams_helpdesk:inbound_message_parse
code_hash: sha256:dab356e11963aeb7ad5feb608cfd18bc6b620f908047f81907c04f3ae3cb6249
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `MailThread.message_parse` (`models/mail_thread_untrusted.py`)

Read 2026-11-11 (anchor sweep, ADR 0090). Tier 1.

- THE override SHALL return the parent's `msg_dict` unchanged and SHALL, as its only side effect, store under that dict's `message_id` (empty string when none) the `inspect_mail` findings, score and removed count, the `ses_verdicts` and the `mail_signals` with `reply_headers` set by `_with_verified_reply_headers`; it SHALL add no key to `msg_dict`, since Odoo refuses an unknown one. HOLDS.
