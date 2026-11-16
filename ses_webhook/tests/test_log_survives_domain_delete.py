# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Webhook audit rows are kept when their domain is deleted (Bruce, 2026-11-16)."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestLogSurvivesDomainDelete(HamsTransactionCase):
    # [@ANCHOR: ses_webhook:test_log_survives_domain_delete]
    def test_log_survives_domain_delete(self):
        # Tests [@ANCHOR: ses_webhook:log_compute_domain_name]
        # Tests [@ANCHOR: ses_webhook:log_compute_company_id]
        domain = self.env["ses.webhook.domain"].create({"name": "keep-logs.example.com"})
        log = self.env["ses.webhook.log"].create(
            {"name": "msg-keep-1", "domain_id": domain.id, "status": "rejected_signature", "payload_type": "Notification"}
        )
        company = log.company_id
        self.assertEqual(log.domain_name, "keep-logs.example.com")
        domain.unlink()
        log.invalidate_recordset()
        self.assertTrue(log.exists(), "the rejected-signature audit row is kept")
        self.assertFalse(log.domain_id)
        self.assertEqual(log.domain_name, "keep-logs.example.com")
        self.assertEqual(log.company_id, company, "a kept row stays scoped to its company")
