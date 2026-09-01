# Compliance Footer in Email Notifications

Every outbound notification email (via Odoo's own `mail.mail_notification_layout`/
`mail.mail_notification_light` templates) needs a footer identifying the sending
organization, configurable per deployment rather than hardcoded.

## Epic: Organization Identification in Notification Emails

* **Story:** As a recipient of a system notification email, I want to see which organization
  sent it, so I can tell a legitimate hams.com notification from anything else landing in my
  inbox.
    * **BDD Criteria:**
        * *Given* `hams_base.compliance_org_name` is configured (or left unset, falling back to
          "HAMS Organization")
        * *When* any notification email renders through Odoo's stock notification layout
        * *Then* the compliance footer renders with the configured organization name, without
          raising a `KeyError` -- a real, previously-shipped bug: the xpath referenced a bare
          `env` variable that `_notify_by_email_render_layout()`'s actual render context never
          provides, crashing every real notification email until fixed to use `company.env[...]`
          instead (`company` is guaranteed present in this render context).
          *(Reference: [@ANCHOR: hams_base:mail_templates])*
