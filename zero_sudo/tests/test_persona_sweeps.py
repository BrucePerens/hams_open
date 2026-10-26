# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.tests.common import tagged

from . import common
from . import test_persona_support as personas_module

# Record rules of this repository that compare a record with an optional attribute of the user, found to hide records
# from a user whose attribute is empty. Deliberate: the strictness is the point. Open: not yet reviewed (a list of
# findings, not a verdict). Each entry is "module.rule_xmlid": ["attribute", ...]; the test fails when a rule appears
# that is in neither list, and when a listed rule no longer collapses, so the lists cannot go stale.
DELIBERATE_STRICT = {
    "pager_duty.pager_check_website_company_rule": ["website_id"],
    "pager_duty.pager_log_file_website_company_rule": ["website_id"],
    "pager_duty.pager_log_pattern_website_company_rule": ["website_id"],
}
OPEN_FINDINGS = {
    # A portal member with no website (all 143 in production) sees only their own tickets that have no website either.
    "hams_helpdesk.rule_helpdesk_ticket_portal": ["website_id"],
    # Found by the first run of this sweep over every module, 2026-10-26; not reviewed. Each compares a record's website
    # with user.website_id.id for a backup administrator, a portal user or a link owner who has no website.
    "backup_management.rule_backup_config_multi_tenant": ["website_id"],
    "backup_management.rule_backup_job_multi_tenant": ["website_id"],
    "backup_management.rule_backup_latest_snapshot_view_multi_tenant": ["website_id"],
    "backup_management.rule_backup_snapshot_multi_tenant": ["website_id"],
    "binary_downloader.rule_binary_tenant_link_multi_tenant": ["website_id"],
}


@tagged("post_install", "-at_install")
class TestPersonaSweeps(common.HamsTransactionCase):
    """Checks that need no per-feature work: they run over every cron and record rule that is installed."""

    # Tests [@ANCHOR: zero_sudo:persona_cron_sweep]
    def test_every_installed_cron_is_allowed_to_run_its_action_as_its_own_user(self):
        refused = personas_module.cron_sweep(self.env)
        self.assertEqual(refused, [], "Odoo would refuse these crons before running them:\n" + "\n".join(refused))

    # Tests [@ANCHOR: zero_sudo:persona_rule_sweep]
    # Tests [@ANCHOR: zero_sudo:persona_repository_modules]
    def test_no_record_rule_hides_records_from_a_user_whose_attribute_is_empty_unless_listed(self):
        found = personas_module.rule_sweep(self.env, "zero_sudo")
        known = {**DELIBERATE_STRICT, **OPEN_FINDINGS}
        installed_known = {name: attrs for name, attrs in known.items() if self.env.ref(name, raise_if_not_found=False)}
        self.assertEqual(
            found,
            installed_known,
            "A rule compares a record with an optional attribute of the user (usually website_id), which is empty for "
            "every production user. Make the rule null-safe, or list it with the reason in DELIBERATE_STRICT.",
        )
