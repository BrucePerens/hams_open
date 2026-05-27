# -*- coding: utf-8 -*-
import logging
import odoo.tests
from odoo.tools import mute_logger
from odoo.addons.hams_test.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)


@odoo.tests.common.tagged("post_install", "-at_install")
class TestUserWebsitesUITours(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.user_test = self.env["res.users"].create(
            {
                "name": "Tour User",
                "login": "touruser",
                "password": "touruser",
                "website_slug": "touruser",
                "lang": "en_US",
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
        self.page = self.env["website.page"].create(
            {
                "url": f"/{self.user_test.website_slug}/home",
                "name": "Tour Page",
                "type": "qweb",
                "arch": '<t name="Tour Page" t-name="tour"><t t-call="website.layout"><div>Tour Content<t t-call="user_websites.report_violation_snippet"/></div></t></t>',
                "owner_user_id": self.user_test.id,
                "website_published": True,
            }
        )
        # Commit to ensure visibility to browser requests and headless workers
        self.env.cr.commit()

    def tearDown(self):
        # Pre-fetch outside the loop to avoid N+1 DB LOCK per ADR-0022
        dynamic_users = self.env["res.users"].search([
            ("login", "in", ["sitetour", "blogtour"])
        ])
        dynamic_pages = self.env["website.page"].search([
            ("url", "in", ["/sitetour/home", "/blogtour/blog"])
        ])
        dynamic_blogs = self.env["blog.blog"].search([
            ("name", "ilike", "Tour User")
        ])
        visitors = self.env["website.visitor"].search([]) if "website.visitor" in self.env else None
        tracks = self.env["website.track"].search([]) if "website.track" in self.env else None

        # Explicit resilient cleanup to prevent website_visitor/website_track pollution
        # Absorbs SerializationFailures if Werkzeug threads are still closing
        for attempt in range(5):
            try:
                with self.env.cr.savepoint(), mute_logger("odoo.sql_db"), mute_logger("odoo.models.unlink"):
                    if visitors and visitors.exists(): visitors.unlink()
                    if tracks and tracks.exists(): tracks.unlink()
                    if getattr(self, 'page', False) and self.page.exists():
                        self.page.unlink()
                    if getattr(self, 'user_test', False) and self.user_test.exists():
                        self.user_test.unlink()

                    # Clean up any records dynamically created during headless tours
                    if dynamic_users.exists():
                        dynamic_users.unlink()

                    if dynamic_pages.exists():
                        dynamic_pages.unlink()

                    if dynamic_blogs.exists():
                        dynamic_blogs.unlink()
                break
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Resilient cleanup encountered exception: %s", e)

        self.env.cr.commit()
        super().tearDown()

    def test_02_toast_notifications_tour(self):
        # Tests [@ANCHOR: test_tour_toast_notifications]
        self.url_open("/?report_submitted=1")
        self.start_tour("/?report_submitted=1&debug=1", "toast_notifications_tour")

    def test_03_gdpr_privacy_tour(self):
        # Tests [@ANCHOR: test_tour_gdpr_privacy]
        self.authenticate(self.user_test.login, "touruser")
        self.url_open("/my/privacy")

        # Adding a minor delay allows Owl components to hydrate in constrained VM environments
        self.start_tour("/my/privacy?debug=1", "gdpr_privacy_tour", login=self.user_test.login, step_delay=100)

    def test_04_moderation_appeal_tour(self):
        # Tests [@ANCHOR: test_tour_moderation_appeal]
        # Tests [@ANCHOR: UX_SUBMIT_APPEAL]
        self.user_test.is_suspended_from_websites = True
        self.env.cr.commit()

        self.authenticate(self.user_test.login, "touruser")
        self.url_open("/my/home")

        self.start_tour("/my/home?debug=1", "moderation_appeal_tour", login=self.user_test.login)

    def test_05_create_site_tour(self):
        # Tests [@ANCHOR: test_tour_create_site]
        user_no_site = self.env["res.users"].create(
            {
                "name": "Site Tour User",
                "login": "sitetour",
                "password": "sitetour",
                "website_slug": "sitetour",
                "lang": "en_US",
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
        self.env.cr.commit()

        self.authenticate(user_no_site.login, "sitetour")
        self.url_open("/sitetour/home")

        self.start_tour("/sitetour/home?debug=1", "create_site_tour", login=user_no_site.login, step_delay=100)

    def test_06_create_blog_tour(self):
        # Tests [@ANCHOR: test_tour_create_blog]
        user_no_blog = self.env["res.users"].create(
            {
                "name": "Blog Tour User",
                "login": "blogtour",
                "password": "blogtour",
                "website_slug": "blogtour",
                "lang": "en_US",
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
        self.env.cr.commit()

        self.authenticate(user_no_blog.login, "blogtour")
        self.url_open("/blogtour/blog")

        self.start_tour("/blogtour/blog?debug=1", "create_blog_tour", login=user_no_blog.login)

    def test_07_community_directory_tour(self):
        # Tests [@ANCHOR: test_tour_community_directory]
        self.url_open("/community")
        self.start_tour("/community?debug=1", "community_directory_tour")

    def test_08_frontend_misc_tour(self):
        # Tests [@ANCHOR: test_tour_frontend_misc]
        self.authenticate(self.user_test.login, "touruser")
        self.url_open("/user-websites/documentation")

        self.start_tour("/user-websites/documentation?debug=1", "frontend_misc_tour", login=self.user_test.login)

# EOF - Patch applied successfully for timeout and DB pollution
