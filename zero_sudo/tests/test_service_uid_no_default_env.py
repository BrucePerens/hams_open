# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
from odoo import api
from odoo.orm.environments import Transaction
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

SERVICE_XMLID = "zero_sudo.mail_service_internal"


@tagged("post_install", "-at_install")
class TestGetServiceUidWithoutDefaultEnv(HamsTransactionCase):
    """Tests [@ANCHOR: zero_sudo:COMM_get_service_uid_no_transaction_flush].

    A public request resolves a service account (tenant_sites' ir.http._match) before authentication has
    given its transaction a user, so the transaction has no `default_env`. The savepoint the lookup used
    to open flushed the transaction, and Odoo answered with the warning "Missing default_env, flushing as
    public user" on every such request."""

    def test_01_lookup_before_the_transaction_has_a_user_logs_no_default_env_warning(self):
        # [@ANCHOR: zero_sudo:COMM_test_get_service_uid_no_transaction_flush]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_get_service_uid_no_transaction_flush]
        cr = self.env.cr
        original = cr.transaction
        fresh = Transaction(self.registry)
        cr.transaction = fresh
        try:
            # What Request._serve_db builds for a visitor with no session: uid None.
            env = api.Environment(cr, None, {})
            self.assertIsNone(
                fresh.default_env,
                "[!] DIAGNOSTIC FOR AI: the setup must reproduce a transaction with no default_env, "
                "or this test proves nothing.",
            )
            with self.assertNoLogs("odoo.api", "WARNING"):
                uid = env["zero_sudo.security.utils"]._get_service_uid(SERVICE_XMLID)
            self.assertTrue(uid, "The lookup must still resolve the service account.")
        finally:
            cr.transaction = original

    def test_02_lookup_still_flushes_pending_writes(self):
        utils = self.env["zero_sudo.security.utils"]
        uid = utils._get_service_uid(SERVICE_XMLID)
        partner = self.env["res.users"].browse(uid).partner_id
        partner.write({"comment": "pending write that the lookup must flush"})
        self.assertTrue(self.env.transaction.field_dirty, "setup: the write must be pending")
        utils._get_service_uid(SERVICE_XMLID)
        self.assertFalse(
            self.env.transaction.field_dirty,
            "[!] DIAGNOSTIC FOR AI: _get_service_uid() must flush pending ORM writes before the "
            "stored procedure reads the service user's row.",
        )
