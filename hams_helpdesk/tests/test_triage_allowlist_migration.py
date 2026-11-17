# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""An installed database's noupdate AI triage allow-list rule is brought to the current domain."""
import importlib.util
import os

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

OLD_DOMAIN = "[('company_id', 'in', company_ids), ('ticket_type', 'in', ['general', 'hams_local_relay'])]"


@tagged("post_install", "-at_install", "standard")
class TestTriageAllowlistMigration(HamsTransactionCase):
    def test_migration_lets_the_triage_account_see_a_server_exception_ticket(self):
        module_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        path = os.path.join(
            module_dir, "migrations", "1.3", "post-ai-triage-allowlist-includes-server-exception.py"
        )
        spec = importlib.util.spec_from_file_location("hams_helpdesk_migration_1_3", path)
        migration = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(migration)

        rule = self.env.ref("hams_helpdesk.rule_helpdesk_ticket_ai_triage_external_allowlist")
        self.assertEqual(rule.domain_force, migration.DOMAIN, "the shipped XML and the migration agree")
        rule.write({"domain_force": OLD_DOMAIN})
        agent = self.env.ref("hams_helpdesk.user_ai_triage_service")
        ticket = self.env["hams_helpdesk.ticket"].create(
            {"name": "Unit failed", "description": "<p>x</p>", "ticket_type": "server_exception"}
        )
        search = lambda: self.env["hams_helpdesk.ticket"].with_user(agent).search([("id", "=", ticket.id)])
        self.assertFalse(search(), "with the old domain the agent cannot see it (the production bug)")

        migration.migrate(self.env.cr, "1.2")
        rule.invalidate_recordset()
        self.env.registry.clear_cache()
        self.assertEqual(rule.domain_force, migration.DOMAIN)
        self.assertTrue(search(), "after the migration the agent sees a New server_exception ticket")
