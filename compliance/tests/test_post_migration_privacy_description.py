# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import importlib.util
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

PATH = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "migrations", "1.5", "post-migration.py")


@tagged("post_install", "-at_install")
class TestPostMigrationPrivacyDescription(HamsTransactionCase):
    def _migration(self):
        spec = importlib.util.spec_from_file_location("compliance_post_migration_1_5", PATH)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_old_text_is_reworded_and_custom_text_is_kept(self):
        # Tests [@ANCHOR: compliance:post_migration_privacy_description]
        module = self._migration()
        doc = self.env.ref("compliance.doc_privacy_policy")
        self.assertEqual(doc.description, module.NEW)  # fresh install ships the new wording
        self.env.cr.execute("UPDATE compliance_document SET description = %s WHERE id = %s", (module.OLD, doc.id))
        doc.invalidate_recordset(["description"])
        module.migrate(self.env.cr, "1.4")
        doc.invalidate_recordset(["description"])
        self.assertEqual(doc.description, module.NEW)
        self.env.cr.execute("UPDATE compliance_document SET description = 'Our own words' WHERE id = %s", (doc.id,))
        module.migrate(self.env.cr, "1.4")
        doc.invalidate_recordset(["description"])
        self.assertEqual(doc.description, "Our own words")
