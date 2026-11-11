# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""What the three test base classes do in `setUpClass`, and what `RealTransactionCase.authenticate` commits.

Each test lives in a class that really inherits the base class, so the assertions look at what that
class's own `setUpClass` left behind: a per-class daemon list, a valid test crypto key, the frozen
`web.base.url`, the TLS proxy in front of the test server, and a committed (not savepointed) row."""
import os
import socket

import odoo
from cryptography.fernet import Fernet

from odoo.tests.common import HOST, HttpCase, get_db_name, tagged

from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestHamsTransactionCaseSetUpClass(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:hams_transaction_case_setup_class]
    def test_set_up_class_gives_the_class_its_own_daemon_list_a_valid_key_and_the_frozen_base_url(self):
        self.assertIsNot(
            type(self)._active_daemons, HamsTransactionCase._active_daemons,
            "each class gets its own list, not the one the base class body created",
        )
        self.assertEqual(type(self)._active_daemons, [])
        Fernet(type(self)._hams_test_crypto_key.encode("utf-8"))  # raises unless it is a real Fernet key
        self.assertIn("HAMS_CRYPTO_KEY", os.environ)
        params = self.env["ir.config_parameter"]
        self.assertEqual(params.get_param("web.base.url"), "https://hams.com")
        self.assertEqual(params.get_param("web.base.url.freeze"), "1")


@tagged("post_install", "-at_install")
class TestHamsHttpCaseSetUpClass(HamsHttpCase):
    # Tests [@ANCHOR: zero_sudo:hams_http_case_setup_class]
    def test_set_up_class_starts_the_tls_proxy_and_gives_the_class_a_valid_key(self):
        cls = type(self)
        self.assertIsNone(cls._socat_proc.poll(), "the TLS proxy started and is still running")
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.settimeout(5.0)
            self.assertEqual(probe.connect_ex((HOST, 8443)), 0, "something accepts connections on the proxy port")
        Fernet(cls._hams_test_crypto_key.encode("utf-8"))
        params = self.env["ir.config_parameter"]
        self.assertEqual(params.get_param("web.base.url.freeze"), "1")


@tagged("post_install", "-at_install")
class TestRealTransactionCaseSetUpAndAuthenticate(RealTransactionCase):
    def _committed_value(self, key):
        """The value another database connection sees, so only a real commit shows."""
        with odoo.sql_db.db_connect(get_db_name()).cursor() as other:
            other.execute("SELECT value FROM ir_config_parameter WHERE key = %s", (key,))
            row = other.fetchone()
        return row[0] if row else None

    # Tests [@ANCHOR: zero_sudo:real_transaction_case_setup_class]
    def test_set_up_class_commits_the_base_url_rows_on_a_real_connection(self):
        self.assertEqual(self._committed_value("web.base.url.freeze"), "1")

    # Tests [@ANCHOR: zero_sudo:real_transaction_case_authenticate]
    def test_authenticate_commits_the_test_cursor_after_signing_in(self):
        key = "zero_sudo.authenticate_probe"
        self.env["ir.config_parameter"].set_param(key, "written before sign-in")
        self.assertIsNone(self._committed_value(key), "still inside the test's own transaction")
        signed_in = self.safe_patch_object(HttpCase, "authenticate", return_value="session")
        self.assertEqual(self.authenticate("admin", "admin"), "session", "the session is handed back")
        signed_in.assert_called_once_with("admin", "admin")
        self.assertEqual(self._committed_value(key), "written before sign-in", "signing in committed the cursor")
