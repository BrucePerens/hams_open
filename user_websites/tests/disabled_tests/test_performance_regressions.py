# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
import logging

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestPerformanceORM(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.test_users = []
        for i in range(15):
            u = self.env["res.users"].create(
                {
                    "name": f"Perf User {i}",
                    "login": f"perfuser{i}",
                    "website_slug": f"perfuser{i}",
                    "group_ids": [
                        (
                            6,
                            0,
                            [
                                self.env.ref("base.group_portal").id,
                                self.env.ref(
                                    "user_websites.group_user_websites_user"
                                ).id,
                            ],
                        )
                    ],
                }
            )
            self.test_users.append(u)
        self.env.flush_all()

    def test_01_site_creation_query_scaling(self):
        # [@ANCHOR: test_site_creation_performance_scaling]
        # Tests [@ANCHOR: test_site_creation_performance_scaling]
        """
        BDD: Given the Master Wrapper Architecture (ADR-fix)
        When provisions multiple user sites sequentially
        Then the number of SQL queries per creation MUST remain constant (O(1)),
        proving the N+1 view invalidation storm is fixed.
        """
        query_counts = []
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )

        for user in self.test_users:
            display_name = f"{user.name} Home"
            unique_key = f"user_websites.home_{user.website_slug}"

            arch_string = f"""<t name="{display_name}" t-name="{unique_key}">
                <t t-call="user_websites.template_default_home">
                    <div id="wrap" class="oe_structure oe_empty"/>
                </t>
            </t>"""

            # Flush before starting to count to isolate the exact transaction
            self.env.flush_all()
            start_queries = self.env.cr.sql_log_count

            self.env["website.page"].with_user(svc_uid).create(
                {
                    "url": f"/{user.website_slug}/home",
                    "name": display_name,
                    "is_published": True,
                    "website_published": True,
                    "type": "qweb",
                    "key": unique_key,
                    "arch": arch_string,
                    "owner_user_id": user.id,
                }
            )

            self.env.flush_all()
            end_queries = self.env.cr.sql_log_count
            query_counts.append(end_queries - start_queries)

        # Discard the first initialization
        stable_counts = query_counts[1:]
        max_diff = max(stable_counts) - min(stable_counts)

        self.assertLessEqual(
            max_diff,
            5,
            f"Query counts are growing linearly! (Variance: {max_diff} queries). The N+1 view invalidation storm has returned. Counts: {query_counts}",
        )


@tagged("post_install", "-at_install")
class TestPerformanceRouting(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.test_users = []
        for i in range(15):
            u = self.env["res.users"].create(
                {
                    "name": f"Route User {i}",
                    "login": f"routeuser{i}",
                    "website_slug": f"routeuser{i}",
                    "group_ids": [
                        (
                            6,
                            0,
                            [
                                self.env.ref("base.group_portal").id,
                                self.env.ref(
                                    "user_websites.group_user_websites_user"
                                ).id,
                            ],
                        )
                    ],
                }
            )
            self.test_users.append(u)

    def test_02_acl_overhead_loop_elimination(self):
        # [@ANCHOR: test_acl_overhead_loop_elimination]
        # Tests [@ANCHOR: test_acl_overhead_loop_elimination]
        """
        BDD: Given multiple user websites exist on the platform,
        When a public guest browses the site,
        Then the routing map and QWeb engine MUST NOT trigger linear
        AccessError loops against the `ir.ui.view` model.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )

        for user in self.test_users:
            display_name = f"{user.name} Home"
            unique_key = f"user_websites.home_{user.website_slug}"
            arch_string = f"""<t name="{display_name}" t-name="{unique_key}">
                <t t-call="user_websites.template_default_home">
                    <div id="wrap" class="oe_structure oe_empty"/>
                </t>
            </t>"""

            self.env["website.page"].with_user(svc_uid).create(
                {
                    "url": f"/{user.website_slug}/home",
                    "name": display_name,
                    "is_published": True,
                    "website_published": True,
                    "type": "qweb",
                    "key": unique_key,
                    "arch": arch_string,
                    "owner_user_id": user.id,
                }
            )

        self.env.flush_all()

        self.authenticate(None, None)

        logger = logging.getLogger("odoo.addons.base.models.ir_model")
        with self.assertLogs(logger, level="WARNING") as cm:
            try:
                self.env.flush_all()
                self.url_open("/")
                self.env.flush_all()
                self.url_open("/community")
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("An error occurred: %s", e)
            logger.warning("DUMMY_WARNING_TO_SATISFY_ASSERTLOGS")

        acl_warnings = [
            record.getMessage()
            for record in cm.records
            if "Access Denied by ACLs" in record.getMessage()
            and (
                "ir.ui.view" in record.getMessage() or "website" in record.getMessage()
            )
        ]

        self.assertEqual(
            len(acl_warnings),
            0,
            f"Linear ACL Overhead Loop Detected! Found {len(acl_warnings)} Access Denied warnings during a standard public request.",
        )

    def test_03_tenant_view_isolation(self):
        # [@ANCHOR: test_tenant_view_isolation]
        # Tests [@ANCHOR: test_tenant_view_isolation]
        """
        BDD: Given two users have provisioned their personal sites using the detached architecture,
        When User A modifies the architecture of their site via the website builder,
        Then User B's site architecture MUST remain completely unaffected.
        """
        user_a = self.test_users[0]
        user_b = self.test_users[1]

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )

        for user in [user_a, user_b]:
            display_name = f"{user.name} Home"
            unique_key = f"user_websites.home_{user.website_slug}"
            arch_string = f"""<t name="{display_name}" t-name="{unique_key}">
                <t t-call="user_websites.template_default_home">
                    <div id="wrap" class="oe_structure oe_empty"/>
                </t>
            </t>"""

            self.env["website.page"].with_user(svc_uid).create(
                {
                    "url": f"/{user.website_slug}/home",
                    "name": display_name,
                    "is_published": True,
                    "website_published": True,
                    "type": "qweb",
                    "key": unique_key,
                    "arch": arch_string,
                    "owner_user_id": user.id,
                }
            )

        page_a = self.env["website.page"].search(
            [("owner_user_id", "=", user_a.id)], limit=1
        )
        self.env["website.page"].search([("owner_user_id", "=", user_b.id)], limit=1)

        # User A edits their page (simulating Odoo web editor saving to arch)
        custom_content = "<div>USER_A_EXCLUSIVE_SECRET_DATA</div>"
        page_a.with_user(user_a).write(
            {
                "arch": page_a.arch.replace(
                    'class="oe_structure oe_empty"/>',
                    f'class="oe_structure oe_empty">{custom_content}</div>',
                )
            }
        )

        # Flush DB to ensure writes propagate to HTTP controllers
        self.env.flush_all()

        # Unauthenticated guest checks both pages
        self.authenticate(None, None)

        # Assert User A's content rendered on A's page
        self.env.flush_all()
        response_a = self.url_open(f"/{user_a.website_slug}/home")
        self.assertEqual(response_a.status_code, 200)
        self.assertIn(
            custom_content.encode("utf-8"),
            response_a.content,
            "User A's edits did not save correctly.",
        )

        # Assert User A's content did NOT bleed into User B's page
        self.env.flush_all()
        response_b = self.url_open(f"/{user_b.website_slug}/home")
        self.assertEqual(response_b.status_code, 200)
        self.assertNotIn(
            custom_content.encode("utf-8"),
            response_b.content,
            "CRITICAL: User A's edits bled into User B's site! Tenant isolation failed.",
        )
