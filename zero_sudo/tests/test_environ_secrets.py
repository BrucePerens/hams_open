# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The web process's /proc/self/environ carries none of the variables of a group moved to the secret store."""

import os
import tempfile

from odoo.tests.common import TransactionCase, tagged

from odoo.addons.zero_sudo.daemon import environ_secrets as es
from odoo.addons.zero_sudo.models import module_secrets as ms
from odoo.addons.zero_sudo.tests.common import SafePatchMixin


@tagged("post_install", "-at_install")
class TestEnvironSecrets(TransactionCase, SafePatchMixin):

    def _environ_file(self, entries):
        handle = tempfile.NamedTemporaryFile(delete=False)
        self.addCleanup(os.unlink, handle.name)
        handle.write(b"\0".join(entries) + b"\0")
        handle.close()
        return handle.name

    # Tests [@ANCHOR: zero_sudo:environ_secrets_names_present]
    def test_names_present_reports_names_only(self):
        path = self._environ_file([b"PATH=/usr/bin", b"SMTP_PASS=test-value", b"GEMINI_API_KEY=", b"HAMS_CRYPTO_KEY=x"])
        self.assertEqual(es.names_present(["mail", "ai", "stripe"], path), ["GEMINI_API_KEY", "SMTP_PASS"])
        self.assertEqual(es.names_present(["cloudflare"], path), [])
        self.assertEqual(es.names_present([], path), [])
        with self.assertRaises(OSError):
            es.names_present(["ai"], "/nonexistent/environ")

    # Tests [@ANCHOR: zero_sudo:environ_secrets_names_present]
    def test_this_process_carries_none_of_the_variables_of_a_group_it_serves_from_the_store(self):
        """The acceptance check of the rollout: on a host whose option lists groups, the running web process has
        none of those groups' old environment variables. Lists nothing, and so passes, where no group is listed."""
        self.assertEqual(es.names_present(ms.enabled_groups()), [])
