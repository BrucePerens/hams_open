# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.modules.module import load_script
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


MIGRATION_PATH = "user_websites/migrations/0.4/post-migration.py"
MIGRATION_MODULE = "odoo.upgrade.user_websites.0.4.post-migration"
REPORT_MODEL = "content.violation.report"
OLD_RULES = {
    "content_violation_report_user_rule": (
        "base.group_portal",
        "[('reported_by_user_id', '=', user.id)]",
    ),
    "content_violation_report_multi_company_rule": (
        "base.group_user",
        "[('company_id', 'in', company_ids)]",
    ),
    "content_violation_report_admin_rule": (
        "user_websites.group_user_websites_administrator",
        "[(1, '=', 1)]",
    ),
    "content_violation_report_service_account_rule": (
        "user_websites.group_user_websites_service_account",
        "[('company_id', 'in', company_ids)]",
    ),
}
OLD_SEQUENCE = "seq_content_violation_report"
NEW_RULES = (
    "content_moderation.content_violation_report_user_rule",
    "content_moderation.content_violation_report_multi_company_rule",
    "content_moderation.content_violation_report_moderator_rule",
    "content_moderation.content_violation_report_service_account_rule",
)
NEW_SEQUENCE = "content_moderation.seq_content_violation_report"


@tagged("post_install", "-at_install")
class TestMigrationOrphanContentViolation(HamsTransactionCase):
    """Recreates the pre-extraction leftovers hams_prod still carries
    (user_websites-owned, noupdate content.violation.report rules and
    sequence) and runs the real 0.4 post-migration over them."""

    # Tests [@ANCHOR: user_websites_orphan_content_violation_cleanup]

    def setUp(self):
        super().setUp()
        self.migration = load_script(MIGRATION_PATH, MIGRATION_MODULE)
        self.report_model = self.env["ir.model"]._get(REPORT_MODEL)
        imd = self.env["ir.model.data"]
        self.old_rules = self.env["ir.rule"]
        for name, (group_xmlid, domain) in OLD_RULES.items():
            rule = self.env["ir.rule"].create(
                {
                    "name": "Legacy " + name,
                    "model_id": self.report_model.id,
                    "groups": [(4, self.env.ref(group_xmlid).id)],
                    "domain_force": domain,
                }
            )
            imd.create(
                {
                    "module": "user_websites",
                    "name": name,
                    "model": "ir.rule",
                    "res_id": rule.id,
                    "noupdate": True,
                }
            )
            self.old_rules |= rule
        self.old_sequence = self.env["ir.sequence"].create(
            {
                "name": "Content Violation Report",
                "code": REPORT_MODEL,
                "prefix": "RPT/",
                "padding": 5,
                "company_id": False,
                "implementation": "standard",
            }
        )
        imd.create(
            {
                "module": "user_websites",
                "name": OLD_SEQUENCE,
                "model": "ir.sequence",
                "res_id": self.old_sequence.id,
                "noupdate": True,
            }
        )
        self.env.flush_all()

    def _run_migration(self, version="19.0.0.3"):
        self.migration.migrate(self.env.cr, version)
        self.env.registry.clear_cache()
        self.env.invalidate_all()

    def _old_xmlid_count(self):
        names = list(OLD_RULES) + [OLD_SEQUENCE]
        return self.env["ir.model.data"].search_count(
            [("module", "=", "user_websites"), ("name", "in", names)]
        )

    def _pg_sequence_exists(self, seq_id):
        self.env.cr.execute(
            "SELECT 1 FROM pg_class WHERE relkind = 'S' AND relname = %s",
            ("ir_sequence_%03d" % seq_id,),
        )
        return bool(self.env.cr.fetchone())

    def test_removes_exactly_the_orphans(self):
        seq_id = self.old_sequence.id
        self.assertEqual(self._old_xmlid_count(), 5)
        self.assertTrue(self._pg_sequence_exists(seq_id))

        self._run_migration()

        self.assertEqual(self._old_xmlid_count(), 0)
        self.assertFalse(self.old_rules.exists())
        self.assertFalse(self.old_sequence.exists())
        self.assertFalse(self._pg_sequence_exists(seq_id))

        # content_moderation's own replacements are untouched, and the
        # code now resolves to exactly one sequence.
        for xmlid in NEW_RULES:
            rule = self.env.ref(xmlid)
            self.assertEqual(rule.model_id, self.report_model)
        new_sequence = self.env.ref(NEW_SEQUENCE)
        self.assertTrue(self._pg_sequence_exists(new_sequence.id))
        remaining = self.env["ir.sequence"].search(
            [("code", "=", REPORT_MODEL)]
        )
        self.assertEqual(remaining, new_sequence)

        # The administrator keeps cross-company moderation through the
        # implied content_moderation moderator group, not the deleted rule.
        admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator"
        )
        moderator_group = self.env.ref(
            "content_moderation.group_content_moderation_moderator"
        )
        self.assertIn(moderator_group, admin_group.all_implied_ids)

    def test_rerun_and_fresh_install_are_noops(self):
        self._run_migration()
        rules_before = self.env["ir.rule"].search_count([])
        sequences_before = self.env["ir.sequence"].search_count([])

        self._run_migration()
        self._run_migration(version=None)

        self.assertEqual(self.env["ir.rule"].search_count([]), rules_before)
        self.assertEqual(
            self.env["ir.sequence"].search_count([]), sequences_before
        )

    def test_same_name_wrong_target_is_left_alone(self):
        # An xmlid with the right name but pointing at a sequence for a
        # different code is not one of the orphans and must survive.
        self.old_sequence.code = "some.other.code"
        self.env.flush_all()

        self._run_migration()

        self.assertTrue(self.old_sequence.exists())
        self.assertEqual(
            self.env["ir.model.data"].search_count(
                [
                    ("module", "=", "user_websites"),
                    ("name", "=", OLD_SEQUENCE),
                ]
            ),
            1,
        )
        self.assertFalse(self.old_rules.exists())
