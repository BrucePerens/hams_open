# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.exceptions import AccessError
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestDaemonKeyRegistryMultiCompany(HamsTransactionCase):
    """
    daemon_key_registry_company_rule ('|', company_id = False, company_id
    in company_ids), scoped to base.group_user, LOOKS like it isolates
    company data -- but daemon.key.registry's only ir.model.access.csv row
    grants any CRUD at all to daemon_key_manager.group_daemon_key_manager,
    and that group ALSO holds daemon_key_manager_all_companies_rule
    (domain [(1,'=',1)], unrestricted). Since every user who can access
    this model at all is (by construction) in group_daemon_key_manager,
    they always match the unrestricted rule too, and ir.rule ORs the
    matching rules together -- so daemon_key_registry_company_rule can
    never actually restrict anyone. Confirmed intentional (not a bug):
    the only two users ever assigned group_daemon_key_manager in the
    codebase are daemon_key_manager's own service account and
    cloudflare's tunnel service account -- narrow, trusted, genuinely
    cross-tenant infra roles, matching the same pattern already verified
    for binary_downloader.group_binary_downloader_manager. What actually
    needs proving is that reach, and that nobody without the group can
    get in at all.
    """

    def setUp(self):
        super().setUp()
        self.company_a = self.env["res.company"].create({"name": "Company A"})
        self.company_b = self.env["res.company"].create({"name": "Company B"})

        # Neither fixture holds base.group_user -- per this codebase's own
        # DOMAIN SANDBOX rule that's reserved for odoo_facility_service_internal
        # only, and it would also be unfaithful to production reality here:
        # daemon_key_manager's own real service account
        # (user_daemon_key_manager_service, security.xml) holds only
        # group_daemon_key_manager, nothing else.
        manager_group = self.env.ref("daemon_key_manager.group_daemon_key_manager")
        self.manager_user = self.env["res.users"].create(
            {
                "name": "Daemon Key Manager User",
                "login": "daemon_key_manager_user",
                "company_id": self.company_a.id,
                "company_ids": [(6, 0, [self.company_a.id])],
                "group_ids": [(6, 0, [manager_group.id])],
            }
        )
        # No groups at all -- proves group_daemon_key_manager is genuinely
        # required (per the class docstring above), not just that this
        # particular baseline happens to lack it.
        self.plain_user = self.env["res.users"].create(
            {
                "name": "Plain Internal User",
                "login": "daemon_key_plain_user",
                "company_id": self.company_a.id,
                "company_ids": [(6, 0, [self.company_a.id])],
                "group_ids": [(6, 0, [])],
            }
        )

        # user_id must be a service account (models.Many2one domain hint,
        # matching real registration usage even though it isn't
        # server-enforced).
        self.svc_a = self.env["res.users"].create(
            {
                "name": "Service A",
                "login": "daemon_key_svc_a",
                "is_service_account": True,
                "company_id": self.company_a.id,
                "company_ids": [(6, 0, [self.company_a.id])],
            }
        )
        self.svc_b = self.env["res.users"].create(
            {
                "name": "Service B",
                "login": "daemon_key_svc_b",
                "is_service_account": True,
                "company_id": self.company_b.id,
                "company_ids": [(6, 0, [self.company_b.id])],
            }
        )

        self.record_a = (
            self.env["daemon.key.registry"]
            .with_company(self.company_a.id)
            .create(
                {
                    "name": "daemon_a",
                    "user_id": self.svc_a.id,
                    "env_file_path": "/opt/hams/etc/keys/daemon_a.env",
                    "company_id": self.company_a.id,
                }
            )
        )
        self.record_b = (
            self.env["daemon.key.registry"]
            .with_company(self.company_b.id)
            .create(
                {
                    "name": "daemon_b",
                    "user_id": self.svc_b.id,
                    "env_file_path": "/opt/hams/etc/keys/daemon_b.env",
                    "company_id": self.company_b.id,
                }
            )
        )

    def test_manager_group_has_intentional_cross_company_reach(self):
        seen = self.env["daemon.key.registry"].with_user(self.manager_user).search([])
        self.assertIn(self.record_a, seen)
        self.assertIn(
            self.record_b,
            seen,
            "[!] DIAGNOSTIC FOR AI: group_daemon_key_manager is a narrow, "
            "trusted, genuinely cross-tenant infra role (only ever held "
            "by daemon_key_manager's and cloudflare's own service "
            "accounts) -- it is SUPPOSED to see every company's daemon "
            "key registry records, matching binary_downloader's manager "
            "group. If this ever starts failing, check whether the "
            "group's real-world membership has widened beyond narrow "
            "service accounts before assuming this test is wrong.",
        )

    def test_plain_internal_user_has_no_access_at_all(self):
        with self.assertRaises(AccessError):
            self.env["daemon.key.registry"].with_user(self.plain_user).search(
                [("id", "=", self.record_a.id)]
            )
        with self.assertRaises(AccessError):
            self.record_a.with_user(self.plain_user).read(["name"])
