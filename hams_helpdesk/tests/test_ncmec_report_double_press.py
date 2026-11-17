# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Pressing "Report" twice at once files the NCMEC report once (two real database transactions)."""
from unittest.mock import MagicMock

import psycopg2.errors
from odoo.exceptions import UserError
from odoo.sql_db import db_connect
from odoo.tests.common import tagged
from odoo.tools import mute_logger

from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.addons.hams_helpdesk.models.helpdesk_ticket import _CSAM_TICKET_TYPE


@tagged("post_install", "-at_install", "standard")
class TestNcmecReportDoublePress(RealTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_action_report]

    def setUp(self):
        super().setUp()
        self.Ticket = self.env["hams_helpdesk.ticket"]
        ticket = self.Ticket.create({"name": "Flagged QSO", "ticket_type": _CSAM_TICKET_TYPE})
        self.env.cr.commit()
        self.ticket_id = ticket.id
        self.addCleanup(self._drop_ticket)
        utils_model = type(self.env["zero_sudo.security.utils"])
        self.safe_patch_object(
            utils_model,
            "_get_system_param",
            lambda _self, key, default=None: {
                "hams_helpdesk.ncmec_api_base_url": "https://ncmec-api-test.invalid/ispws",
                "hams_helpdesk.ncmec_api_username": "u",
                "hams_helpdesk.ncmec_api_password": "p",
            }.get(key, default),
            create=True,
        )
        response = MagicMock()
        response.read.return_value = b"<reportResponse><reportId>NCMEC-1</reportId></reportResponse>"
        response.__enter__.return_value = response
        self.urlopen = self.safe_patch(
            "odoo.addons.hams_helpdesk.models.helpdesk_ticket._urlopen_ssrf_safe", return_value=response
        )
        self.other = db_connect(self.env.cr.dbname).cursor()
        self.addCleanup(self.other.close)

    def _drop_ticket(self):
        self.env.cr.rollback()
        self.other.rollback()
        self.env.invalidate_all()
        self.Ticket.browse(self.ticket_id).unlink()
        self.env.cr.commit()

    def _press(self):
        self.env.invalidate_all()
        self.Ticket.browse(self.ticket_id).with_user(self.env.ref("base.user_admin")).action_ncmec_report()

    def test_a_press_while_another_is_in_flight_sends_nothing(self):
        """Press A holds the row (it is mid-submission, not committed); press B is refused and calls NCMEC zero times."""
        self.other.execute("SELECT id FROM hams_helpdesk_ticket WHERE id = %s FOR UPDATE", (self.ticket_id,))
        self.env.cr.execute("SET LOCAL lock_timeout = '3s'")
        with self.assertRaises(UserError), mute_logger("odoo.sql_db"):
            self._press()
        self.urlopen.assert_not_called()

    def test_a_press_after_the_other_committed_files_nothing_more(self):
        """B looked at the ticket as pending, then A committed "report_submitted": B must not send."""
        ticket = self.Ticket.browse(self.ticket_id)
        self.assertEqual(ticket.ncmec_report_state, "not_reported")
        self.other.execute(
            "UPDATE hams_helpdesk_ticket SET ncmec_report_state = 'report_submitted' WHERE id = %s", (self.ticket_id,)
        )
        self.other.commit()
        with self.assertRaises(psycopg2.errors.SerializationFailure), mute_logger("odoo.sql_db"):
            ticket.with_user(self.env.ref("base.user_admin")).action_ncmec_report()
        self.urlopen.assert_not_called()
        self.env.cr.rollback()
        with self.assertRaises(UserError):
            self._press()
        self.urlopen.assert_not_called()

    def test_a_single_press_still_files(self):
        self._press()
        self.assertEqual(self.urlopen.call_count, 1)
        self.assertEqual(self.Ticket.browse(self.ticket_id).ncmec_report_state, "report_submitted")
