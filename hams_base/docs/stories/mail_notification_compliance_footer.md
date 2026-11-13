# Compliance Footer in Email Notifications

Every outbound notification email (via Odoo's own `mail.mail_notification_layout`/
`mail.mail_notification_light` templates) needs a footer identifying the sending
organization, configurable per deployment rather than hardcoded.

## Epic: Organization Identification in Notification Emails

* **Story:** As a recipient of a system notification email, I want to see which organization
  sent it, so I can tell a legitimate hams.com notification from anything else landing in my
  inbox.
    * **BDD (behaviour-driven development) Criteria:**
        * *Given* `hams_base.compliance_org_name` is configured (or left unset, falling back to
          the name of the sending company, `company.name`)
        * *When* any notification email renders through Odoo's stock notification layout
        * *Then* the compliance footer renders with the configured organization name, without
          raising a `KeyError` -- a real, previously-shipped bug: the xpath referenced a bare
          `env` variable that `_notify_by_email_render_layout()`'s actual render context never
          provides, crashing every real notification email until fixed to use `company.env[...]`
          instead (`company` is guaranteed present in this render context).
          *(Reference: the `hams_base:mail_templates` entry below)*

## Entries

### Notification footer templates [@ANCHOR: hams_base:mail_templates]

`hams_base/views/mail_templates.xml` defines two QWeb inheritances: `mail_notification_layout_hams` (inherits
`mail.mail_notification_layout`) and `mail_notification_light_hams` (inherits `mail.mail_notification_light`). Each adds
a footer `<div>` inside `<body>` (xpath `//body`, position `inside`) containing: the organization name, the mailing
address, the links "How we use your email" (`/email-policy`) and "Unsubscribe Options" (`/unsubscribe`), and a note that
replies are not read.

The name is `company.env['zero_sudo.security.utils']._get_system_param('hams_base.compliance_org_name') or
company.name`, the address is `hams_base.compliance_mailing_address` or empty; both are set on the Settings page
(`res_config_settings_view_form`). The expression goes through `company.env` because the layout's render context has
`company` but no bare `env`; the earlier version raised `KeyError` for every notification. A missing parameter falls back
to the company name and an empty address and never raises. The text is output with `t-out`, so it is HTML-escaped. The
template is evaluated per message, reading the system parameter with the zero-sudo helper rather than `sudo()`. Tests:
`tests/test_mail_templates.py` and `tests/test_email_policy_links.py`.
