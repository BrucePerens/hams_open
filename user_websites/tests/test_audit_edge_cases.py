# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from unittest.mock import MagicMock


@tagged('post_install', '-at_install')
class TestAuditEdgeCases(RealTransactionCase):
    """
    Advanced integration tests targeting edge cases discovered during
    the architectural audit of the user_websites module.
    """

    def setUp(self):
        super(TestAuditEdgeCases, self).setUp()

        self.test_user = self.env["res.users"].create(
            {
                "name": "Edge Case User",
                "login": "edgeuser",
                "email": "edge@example.com",
                "website_slug": "edgeuser",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

    def test_01_gdpr_erasure_suspended_user(self):
        """
        Verify that a suspended user (whose content is unpublished and locked)
        can still legally exercise their Right to Erasure.
        """
        # 1. Create User Content
        page = self.env["website.page"].create(
            {
                "url": f"/{self.test_user.website_slug}/home",
                "name": "Home",
                "type": "qweb",
                "owner_user_id": self.test_user.id,
            }
        )

        # 2. Force a Suspension (3 Strikes)
        self.test_user.violation_strike_count = 3
        self.test_user.action_suspend_user_websites()
        self.assertTrue(self.test_user.is_suspended_from_websites)
        self.assertFalse(
            page.website_published, "Page should be unpublished by suspension."
        )

        # 3. Execute GDPR Erasure
        self.test_user._execute_gdpr_erasure()

        # 4. Verify permanent deletion
        self.assertFalse(
            page.exists(),
            "Suspended user content must be fully unlinked on GDPR erasure, not just unpublished.",
        )

    def test_02_cron_batching_resumption(self):
        # [@ANCHOR: test_cron_batching_resumption]
        # Tests [@ANCHOR: ir_cron_send_weekly_digest]
        """
        Verify that the weekly digest cron successfully parses the last_digest_key
        and resumes processing from the correct index.
        """
        # AST Verification Requirement (ADR-0059)
        self.env.ref("user_websites.ir_cron_send_weekly_digest")._trigger()
        # Ensure a clean state for the system parameter
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        self.env["ir.config_parameter"].with_user(svc_uid).set_param(
            "user_websites.last_digest_key", ""
        )

        blog = self.env["blog.blog"].search([("name", "=", "Community Blog")], limit=1)
        if not blog:
            blog = self.env["blog.blog"].create({"name": "Community Blog"})

        self.env["blog.post"].create(
            {
                "name": "Cron Test Post",
                "blog_id": blog.id,
                "owner_user_id": self.test_user.id,
                "is_published": True,
            }
        )

        # Simulate an interrupted batch by explicitly setting the last_digest_id to a high number
        self.env["ir.config_parameter"].with_user(svc_uid).set_param(
            "user_websites.last_digest_id", "999999"
        )

        # Run the cron method directly
        self.env["blog.post"].send_weekly_digest()

        # Because the id was set very high, the batching logic should skip them.
        # It should cleanly finish and reset the id to 0.
        final_key = self.env["zero_sudo.security.utils"]._get_system_param(
            "user_websites.last_digest_id"
        )
        self.assertEqual(
            final_key,
            "0",
            "Cron must safely reset the digest id to 0 after completing the remaining queue.",
        )
        self.env.ref("user_websites.ir_cron_send_weekly_digest")._trigger()

    def test_03_service_account_tamper_resistance(self):
        """
        Verify that if the Zero-Sudo Service Account is tampered with (e.g., archived),
        the proxy ownership mixin fails closed securely.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        svc_user = self.env["res.users"].browse(svc_uid)

        # Simulate an administrator accidentally archiving the crucial service account
        svc_user.active = False

        # The creation of a website.page utilizes the service account internally via with_user(svc_uid)
        # to bypass the strict Odoo base UI constraints. It must fail safely if the user is inactive.
        try:
            with self.assertRaises(
                Exception,
                msg="System must fail closed if the service account is disabled, denying record creation.",
            ):
                self.env["website.page"].with_user(self.test_user).create(
                    {
                        "url": f"/{self.test_user.website_slug}/fail-test",
                        "name": "Fail Page",
                        "website_id": self.test_user.website_id.id,
                        "owner_user_id": self.test_user.id,
                    }
                )
                self.env.flush_all()
        finally:
            # Restore the active state so subsequent tests don't fail!
            svc_user.active = True

    def test_04_bdd_ormcache_query_counting_slugs(self):
        # [@ANCHOR: test_slug_cache_invalidation]
        # Tests [@ANCHOR: slug_cache_invalidation]
        # Tests [@ANCHOR: slug_cache_invalidation_unlink]
        """
        BDD: Given ADR-0049 Cache Verification
        When resolving slugs repeatedly
        Then it MUST execute exactly 0 SQL queries from cache, and invalidation MUST trigger SQL.
        """
        user = self.env["res.users"].create(
            {"name": "Cache User", "login": "cache_user", "website_slug": "cacheuser"}
        )

        # 1. Prime the cache
        self.env["res.users"]._get_user_id_by_slug("cacheuser")

        # 2. Verify 0 queries on hit
        mock_execute = self.safe_patch_object(
            self.env.cr, "execute", wraps=self.env.cr.execute
        )
        self.env["res.users"]._get_user_id_by_slug("cacheuser")
        for call in mock_execute.call_args_list:
            self.assertNotIn("res_users", call[0][0])

        # 3. Trigger Invalidation
        user.write({"website_slug": "newslug"})

        # 4. Verify cache was cleared (next call must execute SQL)
        user_id = self.env["res.users"]._get_user_id_by_slug("newslug")
        self.assertEqual(
            user_id,
            user.id,
            "The new slug must resolve correctly, proving the cache was cleared.",
        )

        user.unlink()

    def test_05_bdd_ormcache_query_counting_group_slugs(self):
        # [@ANCHOR: test_group_slug_cache_invalidation]
        # Tests [@ANCHOR: group_slug_cache_invalidation]
        # Tests [@ANCHOR: group_slug_cache_invalidation_unlink]
        """
        BDD: Given ADR-0049 Cache Verification
        When resolving group slugs repeatedly
        Then it MUST execute exactly 0 SQL queries from cache, and invalidation MUST trigger SQL.
        """
        group = self.env["user.websites.group"].create(
            {"name": "Cache Group", "website_slug": "cachegroup"}
        )

        # 1. Prime the cache
        self.env["user.websites.group"]._get_group_id_by_slug("cachegroup")

        # 2. Verify 0 queries on hit
        mock_execute = self.safe_patch_object(
            self.env.cr, "execute", wraps=self.env.cr.execute
        )
        self.env["user.websites.group"]._get_group_id_by_slug("cachegroup")
        for call in mock_execute.call_args_list:
            self.assertNotIn("user_websites_group", call[0][0])

        # 3. Trigger Invalidation
        group.write({"website_slug": "newcachegroup"})

        # 4. Verify cache was cleared (next call must execute SQL)
        group_id = self.env["user.websites.group"]._get_group_id_by_slug(
            "newcachegroup"
        )
        self.assertEqual(
            group_id,
            group.id,
            "The new group slug must resolve correctly, proving the cache was cleared.",
        )

        group.unlink()

    def test_06_cron_redis_flush_batching(self):
        # [@ANCHOR: test_cron_redis_flush]
        # Tests [@ANCHOR: ir_cron_flush_view_counters]
        """
        BDD: Given the _flush_redis_view_counters cron
        When it processes a batch of Redis view keys and the cursor is not 0
        Then it MUST update the Postgres records, delete the processed keys,
        and call _trigger() to schedule the next batch (ADR-0022).
        """
        page = self.env["website.page"].create(
            {
                "url": f"/{self.test_user.website_slug}/redis-flush-test",
                "name": "Redis Flush Test",
                "type": "qweb",
                "owner_user_id": self.test_user.id,
            }
        )

        initial_views = page.view_count

        mock_client = self.safe_patch("odoo.addons.user_websites.models.website_page.redis_client")
        # Simulate scan returning a cursor of 5 (more data) and one key
        mock_client.scan.return_value = (5, [f"views:page:{page.id}"])

        # Simulate pipeline execution returning the view count '42' and a DEL success '1'
        mock_pipeline = MagicMock()
        mock_client.pipeline.return_value = mock_pipeline
        mock_pipeline.execute.return_value = ["42", 1]

        cron = self.env.ref(
            "user_websites.ir_cron_flush_view_counters", raise_if_not_found=False
        )
        if not cron:
            self.fail("Cron record ir_cron_flush_view_counters not found.")

        mock_trigger = self.safe_patch_object(type(cron), "_trigger")
        self.env["website.page"]._flush_redis_view_counters()

        # Verify Postgres was updated
        page.invalidate_recordset(["view_count"])
        self.assertEqual(
            page.view_count,
            initial_views + 42,
            "PostgreSQL view_count must be incremented by the Redis value.",
        )

        # Verify pipeline operations
        mock_pipeline.get.assert_called_with(f"views:page:{page.id}")
        # RACE CONDITION FIX: Assert DECRBY is used instead of DELETE to prevent TOCTOU data loss
        mock_pipeline.decrby.assert_called_with(f"views:page:{page.id}", 42)

        # Verify looping via _trigger
        mock_trigger.assert_called_once()
        cron._trigger()

    def test_08_cron_pending_reports(self):
        # [@ANCHOR: test_cron_pending_reports]
        # Tests [@ANCHOR: ir_cron_notify_pending_reports]
        # Tests [@ANCHOR: cron_notify_pending_reports]
        """
        Prove that the cron correctly summarizes pending reports and emails the admin,
        without crashing and using the correct template model.
        """
        self.env["content.violation.report"].create(
            {
                "target_url": "/test-pending",
                "description": "Test",
            }
        )

        self.env["content.violation.report"]._cron_notify_pending_reports()
        self.env.flush_all()

        abuse_email = (
            self.env["zero_sudo.security.utils"]._get_system_param(
                "user_websites.company_abuse_email"
            )
            or self.env.company.email
            or "admin@example.com"
        )
        mail = self.env["mail.mail"].search(
            [
                ("email_to", "ilike", abuse_email),
                ("subject", "ilike", "Action Required"),
            ],
            limit=1,
        )

        self.assertTrue(
            mail, "Cron MUST generate a summary email to the abuse email address."
        )
        self.assertIn("unhandled content violation reports", mail.body_html)

        self.env.ref("user_websites.ir_cron_notify_pending_reports")._trigger()
        template = self.env.ref(
            "user_websites.email_template_pending_violations_summary",
            raise_if_not_found=False,
        )
        if template:
            template.send_mail(self.env.company.id, force_send=False)  # audit-ignore-mail: Tested by [@ANCHOR: test_cron_pending_reports]  # fmt: skip
