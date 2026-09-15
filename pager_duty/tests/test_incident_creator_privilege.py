# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# -*- coding: utf-8 -*-
"""Regression guards for pager_duty.user_pager_incident_creator's narrowed grant.

Service-account privilege audit, 2026-09-14 (bug-hunt): this account is the identity
report_incident(), _raise_trend_incident() and message_new() elevate to -- reachable from any
daemon's report, any inbound email to the info@/postmaster@ aliases, and ham_relay_bridge's
theft-case intake. It used to hold group_pager_service, byte-for-byte the same group list as
user_pager_service_internal, so its separate name bought no privilege reduction at all. It now
holds its own group_pager_incident_creator, whose entire grant is read/write/create on
pager.incident. These tests pin that down mechanically and behaviorally.
"""
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import AccessError


@tagged("post_install", "-at_install", "security")
class TestIncidentCreatorPrivilege(RealTransactionCase):

    def setUp(self):
        super().setUp()
        self.creator = self.env.ref("pager_duty.user_pager_incident_creator")
        self.group = self.env.ref("pager_duty.group_pager_incident_creator")

    # [@ANCHOR: test_incident_creator_holds_only_its_own_group]
    def test_creator_holds_only_its_own_pager_group(self):
        self.assertTrue(self.creator.has_group("pager_duty.group_pager_incident_creator"))
        self.assertFalse(self.creator.has_group("pager_duty.group_pager_service"))
        self.assertFalse(self.creator.has_group("pager_duty.group_pager_admin"))
        self.assertFalse(self.creator.has_group("pager_duty.group_pager_mcp_triage_service"))

    # [@ANCHOR: test_incident_creator_group_grant_is_exactly_pager_incident]
    def test_creator_group_grants_exactly_pager_incident_rwc(self):
        acl = self.env["ir.model.access"].search([("group_id", "=", self.group.id)])
        self.assertEqual(acl.mapped("model_id.model"), ["pager.incident"])
        self.assertEqual(
            [(a.perm_read, a.perm_write, a.perm_create, a.perm_unlink) for a in acl],
            [(True, True, True, False)],
        )
        rules = self.env["ir.rule"].search([("groups", "in", self.group.ids)])
        self.assertEqual(rules.mapped("model_id.model"), ["pager.incident"])

    # [@ANCHOR: test_incident_creator_cannot_reach_pager_check]
    # Tests [@ANCHOR: report_incident_rate_limit]
    def test_creator_can_create_incident_but_cannot_touch_checks_or_unlink(self):
        Incident = self.env["pager.incident"].with_user(self.creator)
        incident = Incident.create(
            {
                "name": "INC-AUTO",
                "source": "privilege-audit-test",
                "severity": "low",
                "description": "created directly as the incident creator account",
            }
        )
        self.assertTrue(incident.exists())
        incident.write({"occurrence_count": 2})
        self.assertEqual(incident.occurrence_count, 2)
        with self.assertRaises(AccessError):
            incident.unlink()
        # pager.check (heartbeat monitors, Let's Encrypt domain lists) was reachable under the
        # old shared group and must not be now -- even a bare search.
        with self.assertRaises(AccessError):
            self.env["pager.check"].with_user(self.creator).search([], limit=1)
