# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later

import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from odoo.addons.backup_management.hooks import post_init_hook


@tagged('post_install', '-at_install')
class TestBackupManagementHooks(HamsTransactionCase):
    """post_init_hook() had never been exercised -- same untested-since-
    install gap found and fixed for hams_s3.hooks and
    distributed_redis_cache.hooks: that it actually registers the daemon
    with the right name/xml_id/env_file_path, and is safe to re-run on a
    module upgrade."""

    def test_post_init_hook_registers_the_backup_worker_daemon(self):
        self.env.ref("backup_management.user_backup_service_internal")

        post_init_hook(self.env)

        registry = self.env["daemon.key.registry"].search(
            [("name", "=", "Backup Worker RabbitMQ Consumer")], limit=1
        )
        self.assertTrue(registry, "post_init_hook did not register the Backup Worker daemon")
        self.assertEqual(registry.env_file_path, "/opt/hams/etc/keys/backup_worker.env")
        self.assertTrue(os.path.exists(registry.env_file_path))

    def test_post_init_hook_is_idempotent_on_reinstall(self):
        post_init_hook(self.env)
        post_init_hook(self.env)

        registries = self.env["daemon.key.registry"].search(
            [("name", "=", "Backup Worker RabbitMQ Consumer")]
        )
        self.assertEqual(len(registries), 1)
