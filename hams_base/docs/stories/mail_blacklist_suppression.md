# Queued mail skips blacklisted addresses [@ANCHOR: hams_base:mail_skips_blacklisted_recipients]

SES reports a permanent bounce or a spam complaint and ses_webhook puts the address on `mail.blacklist`. Core Odoo
consults that list only for chatter notifications, so a mail.mail created directly (weekly digest, template mail,
onboarding and consent mail) would still go to that address. `mail.mail._send` in hams_base now drops blacklisted
addresses from `email_to`, `email_cc` and `recipient_ids` just before sending, and marks the mail `exception` (as a refused send is) when none is left.
Test: `test_a_blacklisted_recipient_is_dropped_before_send`.

## `_send` only filters queued mail [@ANCHOR: hams_base:mail_send_filters_outgoing]

`MailMailBlacklist._send` runs the blacklist drop only on mails still in state `outgoing`, then hands the whole set to
core `_send`. A mail already `sent` or `exception` keeps its recipients untouched, so a later re-send call cannot rewrite
the addresses of history that was delivered. Test: `test_send_filters_only_outgoing_mail`.
