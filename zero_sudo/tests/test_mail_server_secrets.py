# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Outgoing mail server passwords from the secret store (group `mail`, zero_sudo/models/mail_server_secrets.py).
Test values only; no connection is opened. Story: zero_sudo/docs/stories/secret_store_tier_a.md ("Module secrets")."""

from unittest.mock import MagicMock

from odoo.exceptions import UserError
from odoo.tests.common import TransactionCase, tagged

from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.models import mail_server_secrets as mss
from odoo.addons.zero_sudo.models import module_secrets as ms
from odoo.addons.zero_sudo.tests.common import SafePatchMixin

DB_PASS = "test-smtp-database-copy"
STORE_PASS = "test-smtp-store-copy"


class _DictBackend:
    label = "test"

    def __init__(self, values):
        self.values = values

    def get(self, name):
        return self.values.get(ss.check_name(name))


@tagged("post_install", "-at_install")
class TestMailServerSecrets(TransactionCase, SafePatchMixin):

    def setUp(self):
        super().setUp()
        self._group(False)
        self.server = self.env["ir.mail_server"].create(
            {"name": "Store test server", "smtp_host": "smtp.invalid", "smtp_user": "u", "smtp_pass": DB_PASS})
        self.store_values = {mss.smtp_store_name(self.server.id): STORE_PASS}
        self.safe_patch_object(
            ms, "module_secret_store", side_effect=lambda: ss.SecretStore([_DictBackend(self.store_values)]))

    def _group(self, on):
        self.safe_patch_object(ms, "enabled_groups", return_value=frozenset({"mail"}) if on else frozenset())

    def _login(self):
        connection = MagicMock()
        self.server._smtp_login__(connection, "u", DB_PASS)
        return connection.login.call_args.args

    def _column(self):
        self.env.flush_all()
        self.env.cr.execute("SELECT smtp_pass FROM ir_mail_server WHERE id = %s", [self.server.id])
        return self.env.cr.fetchone()[0]

    def test_store_name(self):
        # Tests [@ANCHOR: zero_sudo:mail_server_store_name]
        self.assertEqual(mss.smtp_store_name(7), "mail.smtp_pass.7")

    def test_group_off_logs_in_with_the_given_password(self):
        # Tests [@ANCHOR: zero_sudo:mail_server_smtp_login]
        # Tests [@ANCHOR: zero_sudo_mail_server_secrets_ir_mail_server]
        self.assertEqual(self._login(), ("u", DB_PASS))

    def test_group_on_logs_in_with_the_store_password_or_fails(self):
        # Tests [@ANCHOR: zero_sudo:mail_server_smtp_login]
        self._group(True)
        self.assertEqual(self._login(), ("u", STORE_PASS))
        self.store_values.clear()
        with self.assertRaises(ss.SecretNotFound):
            self._login()

    def test_group_on_refuses_a_typed_password(self):
        # Tests [@ANCHOR: zero_sudo:mail_server_create]
        # Tests [@ANCHOR: zero_sudo:mail_server_write]
        self._group(True)
        with self.assertRaises(UserError):
            self.server.write({"smtp_pass": "test-typed"})
        with self.assertRaises(UserError):
            self.env["ir.mail_server"].create({"name": "x", "smtp_host": "h", "smtp_pass": "test-typed"})
        self.server.write({"smtp_pass": False})

    def test_clear_step_empties_the_column_only_when_the_store_has_the_password(self):
        # Tests [@ANCHOR: zero_sudo:mail_server_clear_from_database]
        self._group(True)
        self.store_values.clear()
        with self.assertRaises(ss.SecretNotFound):
            self.env["ir.mail_server"]._hams_mail_clear_smtp_pass_from_database()
        self.assertEqual(self._column(), DB_PASS)
        self.store_values[mss.smtp_store_name(self.server.id)] = STORE_PASS
        self.assertIn(self.server.id, self.env["ir.mail_server"]._hams_mail_clear_smtp_pass_from_database())
        self.assertFalse(self._column())
        self.assertEqual(self._login(), ("u", STORE_PASS))
        self._group(False)
        with self.assertRaises(UserError):
            self.env["ir.mail_server"]._hams_mail_clear_smtp_pass_from_database()
