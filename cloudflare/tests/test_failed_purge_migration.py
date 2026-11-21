# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The 1.10 migration archives stale failed purge rows and deletes none; running it twice gives the same state."""
import importlib.util
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

_MIGRATION = os.path.join(os.path.dirname(__file__), "..", "migrations", "1.10", "post-drop-failed-purges.py")


@tagged("post_install", "-at_install")
class TestFailedPurgeMigration(HamsTransactionCase):
    def test_failed_rows_are_archived_not_deleted_and_rerun_is_stable(self):
        spec = importlib.util.spec_from_file_location("drop_failed_purges", _MIGRATION)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        Queue = self.env["cloudflare.purge.queue"]
        failed = Queue.create({"state": "failed"})
        pending = Queue.create({"state": "pending"})
        Queue.flush_model()
        module.migrate(self.env.cr, "1.9")
        Queue.invalidate_model()
        self.assertEqual(failed.exists().state, "archived")
        self.assertEqual(pending.exists().state, "pending")
        module.migrate(self.env.cr, "1.9")
        Queue.invalidate_model()
        self.assertEqual((failed.state, pending.state), ("archived", "pending"))
