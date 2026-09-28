# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.tests.common import tagged

from . import common


@tagged("post_install", "-at_install")
class TestServiceAccountSecurityNotices(common.HamsTransactionCase):
    def _security_mails(self, user):
        return self.env["mail.mail"].search([("recipient_ids", "in", user.partner_id.ids)])

    def _make(self, login, service):
        return self.env["res.users"].create({
            "name": login,
            "login": login,
            "email": login,
            "is_service_account": service,
            "lang": "en_US",
        })

    # [@ANCHOR: test_a_service_account_is_never_sent_a_security_update_email]
    # Tests [@ANCHOR: zero_sudo:service_account_security_notice_suppressed]
    def test_a_service_account_is_never_sent_a_security_update_email(self):
        service = self._make("svc_notice_test@example.com", True)
        before = self._security_mails(service)
        service._notify_security_setting_update("Security Update: Login Changed", "Your login was changed")
        self.assertEqual(self._security_mails(service), before, "no notice may be queued for a service account")

    # Tests [@ANCHOR: zero_sudo:service_account_security_notice_suppressed]
    def test_a_person_is_still_notified(self):
        person = self._make("person_notice_test@example.com", False)
        before = self._security_mails(person)
        person._notify_security_setting_update("Security Update: Login Changed", "Your login was changed")
        self.assertEqual(len(self._security_mails(person)) - len(before), 1)

    # Tests [@ANCHOR: zero_sudo:service_account_security_notice_suppressed]
    def test_a_mixed_set_notifies_only_the_person(self):
        service = self._make("svc_mixed_test@example.com", True)
        person = self._make("person_mixed_test@example.com", False)
        before_service = self._security_mails(service)
        before_person = self._security_mails(person)
        (service | person)._notify_security_setting_update("Security Update: Password Changed", "Your password was changed")
        self.assertEqual(self._security_mails(service), before_service)
        self.assertEqual(len(self._security_mails(person)) - len(before_person), 1)

    # Tests [@ANCHOR: zero_sudo:service_account_security_notice_suppressed]
    def test_writing_a_service_accounts_login_queues_no_notice(self):
        # The real path an upgrade takes: the XML data writes login on the record.
        service = self._make("svc_write_test@example.com", True)
        before = self._security_mails(service)
        service.write({"login": "svc_write_test_renamed@example.com"})
        self.assertEqual(self._security_mails(service), before)

    # Tests [@ANCHOR: zero_sudo:service_account_security_notice_suppressed]
    def test_an_empty_recordset_is_a_quiet_no_op(self):
        self.assertIsNone(self.env["res.users"].browse()._notify_security_setting_update("s", "c"))
