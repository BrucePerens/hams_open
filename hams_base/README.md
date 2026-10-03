# Hams Base

The `hams_base` module provides foundational security, compliance, and email transport overrides for the Hams Open platform.

## Developer & AI Reference

### Email & DMARC Handling
* **`hams_base.dmarc.report` & `hams_base.dmarc.record`**:
  * Receives mail at the dedicated `dmarc-reports@` alias (`data/mail_alias_data.xml`), which creates `hams_base.dmarc.report` records. This is the address the suggested DMARC record's `rua=` tag (the destination for aggregate reports) points to.
  * Parses incoming DMARC (Domain-based Message Authentication, Reporting and Conformance) RUA (Aggregate) XML reports to analyze email alignment and SPF (Sender Policy Framework) / DKIM (DomainKeys Identified Mail) validation failures. An attachment may be raw `.xml`, `.zip` or `.gz` (decompressed size capped at 50 MB). Each report becomes one `hams_base.dmarc.report` (the published policy, de-duplicated on the report's `report_id`) plus one `hams_base.dmarc.record` per sending source IP with its message count, disposition and DKIM/SPF results.
  * Failure path: a malformed or oversized attachment is logged and skipped, never raised into Odoo's mail gateway. An email with no parseable attachment is still stored, as a report named "Unparsed Email: <subject>".
* **`mail.thread` Overrides**:
  * Overrides the message routing logic for mail addressed to the bounce alias (`mail.bounce.alias`, default `auto-mail-failure`), `not-read@` or `postmaster@`. Vacation replies and auto-responders (subject contains "out of office", "vacation" or "auto-reply") are dropped so they do not create records.
  * Messages to those addresses that mention "unsubscribe" in the subject or body are logged and dropped. No subscription is changed; the sender has to use the `/unsubscribe` page.
  * Recognized bounces continue to Odoo's normal bounce processing. Any other mail to `not-read@` is dropped; other mail to `postmaster@` is routed normally.
* **`res.partner` Overrides**:
  * `_message_receive_bounce()`: Runs after Odoo's own bounce handling for a partner and posts a "Bounce Alert" chatter message, asking the club to reach the member another way, on each club the partner belongs to (the partner's `club_ids` when another module provides that field, otherwise the partner's parent company). A club whose own email is the bouncing address is skipped, and a failure for one club does not stop the others.

### Security Alerts & Configuration
* **`res.users` Overrides**:
  * When a user's email address or login changes, sends a "Security Alert" email to the previous address so the owner can spot an unauthorized change. If sending fails, the change is still saved and the failure is logged. (Password-change notices come from Odoo itself, not from this module.)
* **`res.config.settings`**:
  * Manages compliance settings (organization name and mailing address, shown in the footer `views/mail_templates.xml` adds to Odoo's notification emails) and exposes recommended SPF and DMARC DNS configurations for the instance domain. The SPF record is a template naming an example provider, to be edited. The DMARC record (`p=quarantine`, reports to `dmarc-reports@<domain>`) is shown only when "Enable Custom DMARC" is on. The domain comes from `mail.catchall.domain`, or else `web.base.url`. DKIM records must be taken from the mail provider.

### Website Routes
* `/email-policy`: public page listing the kinds of email the platform sends.
* `/unsubscribe`: public page offering a link to email preferences, or (for a logged-in user) a total account lockout.
* `/unsubscribe/lockout` (logged-in users, POST only): deactivates the user's own account through the `zero_sudo.user_lockout_service_internal` service account and logs the user out. Reversing it requires an administrator.
