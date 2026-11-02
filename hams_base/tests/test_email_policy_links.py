# -*- coding: utf-8 -*-
import os
import re

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged

# The placeholder that used to be the footer's fallback organisation name. Assembled from parts so this file does not contain it.
_PLACEHOLDER = " ".join(("HAMS", "Organization"))
_SKIP_DIRS = {"tests", ".git", "target", "node_modules", "__pycache__", "static"}
_SHIPPED_SUFFIXES = (".xml", ".py", ".csv", ".html", ".md", ".txt", ".json", ".js")


@tagged("post_install", "-at_install")
class TestEmailFooterBrandAndLinks(HamsHttpCase):
    """The footer of every notification email names the company (the single source of the brand) and links to a page that exists."""

    def _open_root(self):
        # hams_base/tests/this_file -> the hams_open checkout that holds hams_base
        return os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

    def test_no_placeholder_organisation_in_any_shipped_file(self):
        # Tests [@ANCHOR: hams_base:mail_templates]
        found = []
        for base, dirs, files in os.walk(self._open_root()):
            dirs[:] = [d for d in dirs if d not in _SKIP_DIRS]
            for name in files:
                if not name.endswith(_SHIPPED_SUFFIXES):
                    continue
                path = os.path.join(base, name)
                try:
                    with open(path, encoding="utf-8", errors="replace") as fh:
                        if _PLACEHOLDER in fh.read():
                            found.append(path)
                except OSError:
                    continue
        self.assertEqual(
            found, [],
            "[!] DIAGNOSTIC FOR AI: a shipped template or data file names the placeholder organisation; "
            "use the company name (company.name) or hams_base.compliance_org_name instead.",
        )

    def _footer_html(self):
        partner = self.env["res.partner"].create({"name": "Footer Recipient", "email": "footer_recipient@example.com"})
        partner.with_context(mail_notify_force_send=False).message_notify(body="Body", subject="Footer check", partner_ids=[partner.id])
        mail = self.env["mail.mail"].search([("recipient_ids", "in", partner.id)], order="id desc", limit=1)
        self.assertTrue(mail, "message_notify() should have queued a mail.mail.")
        # The text a mail client receives, after Odoo has made local links absolute.
        return mail, mail._prepare_outgoing_body()

    def test_footer_uses_company_name_when_no_parameter_is_set(self):
        # Tests [@ANCHOR: hams_base:mail_templates]
        self.env["ir.config_parameter"].with_user(self.env.ref("base.user_admin").id).set_param("hams_base.compliance_org_name", False)
        self.env.company.name = "Footer Test Company"
        _mail, body = self._footer_html()
        self.assertIn("Footer Test Company", body)
        self.assertNotIn(_PLACEHOLDER, body)

    def test_footer_links_are_absolute_and_resolve(self):
        # Tests [@ANCHOR: hams_base:mail_templates]
        # Tests [@ANCHOR: hams_base:email_policy_template]
        # Tests [@ANCHOR: hams_base:unsubscribe_page_template]
        _mail, body = self._footer_html()
        base = self.env["zero_sudo.security.utils"]._get_system_param("web.base.url").rstrip("/")
        hrefs = re.findall(r'href="([^"]+)"', body)
        for tail in ("/email-policy", "/unsubscribe"):
            self.assertIn(base + tail, hrefs, f"[!] DIAGNOSTIC FOR AI: the footer must link to {base}{tail}; links found: {hrefs}")
            response = self.url_open(tail)
            self.assertEqual(response.status_code, 200, f"[!] DIAGNOSTIC FOR AI: the footer link {tail} must resolve (it used to point at /compliance/email-policy, a 404).")
        self.assertNotIn("/compliance/email-policy", body)
