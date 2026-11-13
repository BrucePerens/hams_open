# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The rotation cron's batch of 10 must not be filled for ever by registries that fail on every run."""
import datetime

from odoo import fields
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestCronFailingRegistriesDoNotStarveOthers(RealTransactionCase):
    def test_failing_registries_do_not_block_a_good_one(self):
        # Tests [@ANCHOR: COMM_cron_rotation_logic]
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        model = self.env["daemon.key.registry"].with_user(manager.id)
        user = self.env["res.users"].create(
            {"name": "Starve Svc", "login": "starve_svc", "is_service_account": True}
        )
        now = fields.Datetime.now()
        # The failing ones are the OLDEST, so an order by last_rotated alone puts them first on every run.
        failing = model.create([
            {"name": f"Starve {i:02d}", "user_id": user.id, "env_file_path": f"/opt/hams/etc/keys/starve_{i:02d}.env",
             "last_rotated": now - datetime.timedelta(days=90)}
            for i in range(10)
        ])
        good = model.create({
            "name": "Starve Good", "user_id": user.id, "env_file_path": "/opt/hams/etc/keys/starve_good.env",
            "last_rotated": now - datetime.timedelta(days=70),
        })
        self.env.cr.commit()

        def cleanup():
            (failing | good).unlink()
            user.unlink()
            self.env.cr.commit()

        self.addCleanup(cleanup)

        def fake_rotate(reg, pre_fetched_keys=None):
            if reg.name.startswith("Starve ") and reg.name != "Starve Good":
                raise OSError("key file is not writable")
            reg.last_rotated = fields.Datetime.now()

        self.safe_patch_object(type(model), "_rotate_key_and_write_file", fake_rotate)
        for _run in range(3):
            model._cron_rotate_all_keys()
        self.assertTrue(
            good.last_rotated > now - datetime.timedelta(days=1),
            "ten registries that fail on every run kept the eleventh out of the cron's batch",
        )
        self.assertTrue(all(reg.last_rotation_failed_at for reg in failing))
