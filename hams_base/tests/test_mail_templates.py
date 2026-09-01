# -*- coding: utf-8 -*-
from odoo.tests import common, tagged


@tagged('post_install', '-at_install')
class TestMailTemplates(common.TransactionCase):
    """Real regression test for a genuine, previously-undetected bug: the
    compliance-footer xpath this module injects into mail.mail_notification_layout/
    mail.mail_notification_light (views/mail_templates.xml) referenced a bare
    `env` variable in its `t-out` expressions, but Odoo's own
    _notify_by_email_render_layout() docstring documents the real render_values
    contract for this template as `company, is_discussion, lang, message,
    model_description, record, record_name, signature, subtype,
    tracking_values, website_url` -- no `env` key at all. Every real email
    notification rendered through this layout crashed with
    `QWebError: ... KeyError: 'env'` as a direct consequence. The xpath's own
    "Tested by [@ANCHOR: hams_base:mail_templates]" comment was a false
    claim -- no test for this anchor existed anywhere, which is exactly why
    this went uncaught. Fixed by using `company.env[...]` (`company` is a
    real, guaranteed-present res.company recordset in this render context,
    confirmed against Odoo's own stock template content, which already
    relies on it unconditionally for `company.name`/`company.phone`/etc.).
    """

    # Tests [@ANCHOR: hams_base:mail_templates]
    def test_notification_email_layout_renders_without_env_keyerror(self):
        org_name = "Test Compliance Org Name"
        self.env['ir.config_parameter'].with_user(
            self.env.ref('base.user_admin').id
        ).set_param('hams_base.compliance_org_name', org_name)

        # An email-only recipient (no user_ids) forces the real
        # _notify_by_email path -- an internal-user recipient would only
        # exercise the inbox-notification path, which never renders this
        # template at all.
        partner = self.env['res.partner'].create({
            'name': 'Email Only Recipient',
            'email': 'email_only_recipient@example.com',
        })

        # Must not raise QWebError/KeyError('env').
        self.env['res.partner'].browse(partner.id).message_notify(
            body="Test notification body",
            subject="Test notification subject",
            partner_ids=[partner.id],
        )

        mail = self.env['mail.mail'].search(
            [('recipient_ids', 'in', partner.id)], order='id desc', limit=1
        )
        self.assertTrue(mail, "message_notify() should have queued a real mail.mail record.")
        self.assertIn(org_name, mail.body_html or "")
