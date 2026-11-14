# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""A first register_daemon that loses the race for UNIQUE(name, company).

Odoo runs a request in a REPEATABLE READ transaction, so the loser cannot see the winner's row by searching again. It must ask
Odoo to run the request again (`ConcurrencyError`), not fail with the raw database error. The race is made deterministic: the row
exists, and the first search is blind to it, as a snapshot older than the winner's commit is."""
import os

from odoo.exceptions import ConcurrencyError
from odoo.tests import tagged
from odoo.tools import mute_logger
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

FIRST_FILE = "/opt/hams/etc/keys/race_test.env"
SECOND_FILE = "/opt/hams/etc/keys/race_test_other.env"
DAEMON_NAME = "Race Test Daemon"


@tagged("post_install", "-at_install")
class TestRegisterDaemonRace(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.addCleanup(self._remove_files)
        self._remove_files()
        self.service_user = self.env["res.users"].create(
            {"name": "Race Test Service Account", "login": "race_test_service_account", "is_service_account": True}
        )
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        self.registry_model = self.env["daemon.key.registry"].with_user(manager)

    def _remove_files(self):
        for path in (FIRST_FILE, SECOND_FILE):
            if os.path.lexists(path):
                os.remove(path)

    def test_a_simultaneous_first_register_reruns(self):
        # Tests [@ANCHOR: COMM_register_daemon_logic]
        self.registry_model.register_daemon(DAEMON_NAME, self.service_user.login, FIRST_FILE)
        model_class = type(self.registry_model)
        real_search = model_class.search
        blind = []

        def blind_first_search(records, *args, **kwargs):
            if not blind:
                blind.append(1)
                return records.browse()
            return real_search(records, *args, **kwargs)

        self.safe_patch_object(model_class, "search", blind_first_search)
        with mute_logger("odoo.sql_db"), self.assertRaises(ConcurrencyError):
            self.registry_model.register_daemon(DAEMON_NAME, self.service_user.login, SECOND_FILE)
