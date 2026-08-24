# SPDX-License-Identifier: AGPL-3.0-or-later

import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from odoo.addons.hams_s3.hooks import post_init_hook


@tagged('post_install', '-at_install')
class TestHamsS3Hooks(HamsTransactionCase):
    """hooks.py's own module docstring flags post_init_hook as DRAFT,
    UNVERIFIED and "not exercised end to end" -- but only the ACL-against-
    OCA's-storage_backend half of that is genuinely blocked (that addon
    isn't installed in this sandbox, see hams_s3_security.xml's own header
    comment). The hook function itself -- that it calls
    daemon.key.registry.register_daemon() with the right daemon_name/
    user_xml_id/env_file_path, and that the referenced service account xml
    id actually resolves -- has no dependency on storage_backend at all
    and was simply never tested. distributed_redis_cache/hooks.py's own
    post_init_hook (this module's own explicitly-cited template) had the
    same gap -- see distributed_redis_cache/tests/test_hooks.py.
    """

    def test_post_init_hook_registers_the_s3_manager_daemon(self):
        # Confirms the referenced service account actually exists before
        # calling the hook -- if this ref() fails, the hook itself would
        # raise inside register_daemon(), and this test should say so
        # plainly rather than fail confusingly deeper in the stack.
        self.env.ref("hams_s3.s3_manager_service_internal")

        post_init_hook(self.env)

        registry = self.env["daemon.key.registry"].search(
            [("name", "=", "S3 Storage Manager")], limit=1
        )
        self.assertTrue(registry, "post_init_hook did not register the S3 Storage Manager daemon")
        self.assertEqual(registry.env_file_path, "/opt/hams/etc/keys/s3_manager.env")
        self.assertTrue(os.path.exists(registry.env_file_path))

    def test_post_init_hook_is_idempotent_on_reinstall(self):
        # A module can be reinstalled/upgraded, re-running its post_init_hook
        # -- register_daemon() must not blow up or duplicate the registry
        # entry the second time around.
        post_init_hook(self.env)
        post_init_hook(self.env)

        registries = self.env["daemon.key.registry"].search(
            [("name", "=", "S3 Storage Manager")]
        )
        self.assertEqual(len(registries), 1)
