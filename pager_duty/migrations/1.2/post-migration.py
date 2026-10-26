# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pager Duty 1.2: a pager admin with no website of their own sees the incidents of every website in their companies.

[@ANCHOR: pager_duty:admin_rule_migration]

The admin record rule of pager.incident was compared with `user.website_id.id`, which is empty for an internal user, so
on hams1 the admins could read none of the incidents that carry a website (13 of 36), and the "View Pager Duty Incident"
link in an incident email (`/mail/view`) fell back to the Discuss app. The rules of pager.check, pager.log.file and
pager.log.pattern are left strict on purpose (an admin must not read another website's log files). The rule is in a
`noupdate` block, so an upgrade does not rewrite it; this does.

Plain SQL, matching this platform's zero-sudo rule for migration scripts. Running it again changes nothing further."""

import logging

_logger = logging.getLogger(__name__)

_RULES = ("pager_incident_website_company_rule",)
_DOMAIN = (
    "(['|', ('website_id', '=', False), ('website_id', '=', user.website_id.id)] if user.website_id else []) "
    "+ ['|', ('company_id', '=', False), ('company_id', 'in', company_ids)]"
)


def migrate(cr, version):
    cr.execute(
        """UPDATE ir_rule r SET domain_force = %s
             FROM ir_model_data d
            WHERE d.module = 'pager_duty' AND d.model = 'ir.rule' AND d.res_id = r.id AND d.name IN %s
              AND r.domain_force IS DISTINCT FROM %s""",
        (_DOMAIN, _RULES, _DOMAIN),
    )
    _logger.info("pager_duty 1.2: %s admin incident record rule(s) no longer confine a website-less admin to a website.", cr.rowcount)
