# -*- coding: utf-8 -*-
import odoo.tests


@odoo.tests.common.tagged("post_install", "-at_install")
class TestUserWebsitesUITours(odoo.tests.HttpCase):
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
        self.env["website.page"].create(
            {
                "url": f"/{self.user_test.website_slug}/home",
                "name": "Tour Page",
                "type": "qweb",
                "arch": '<t name="Tour Page" t-name="tour"><t t-call="website.layout"><div>Tour Content</div></t></t>',
                "owner_user_id": self.user_test.id,
                "website_published": True,
            }
        )

    def test_01_violation_report_tour(self):
        # Tests [@ANCHOR: test_tour_violation_report]
        # Access the page as an unauthenticated guest so the Report Violation button is visible
        self.start_tour(f"/{self.user_test.website_slug}/home", "violation_report_tour")

    def test_02_toast_notifications_tour(self):
        # Tests [@ANCHOR: test_tour_toast_notifications]
        self.start_tour("/?report_submitted=1", "toast_notifications_tour")

    def test_03_gdpr_privacy_tour(self):
        # Tests [@ANCHOR: test_tour_gdpr_privacy]
        self.start_tour("/my/privacy", "gdpr_privacy_tour", login=self.user_test.login)

    def test_04_moderation_appeal_tour(self):
        # Tests [@ANCHOR: test_tour_moderation_appeal]
        # Tests [@ANCHOR: UX_SUBMIT_APPEAL]
        self.user_test.is_suspended_from_websites = True
        self.start_tour("/my/home", "moderation_appeal_tour", login=self.user_test.login)

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
        self.start_tour("/sitetour/home", "create_site_tour", login=user_no_site.login)

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
        self.start_tour("/blogtour/blog", "create_blog_tour", login=user_no_blog.login)

    def test_07_community_directory_tour(self):
        # Tests [@ANCHOR: test_tour_community_directory]
        self.start_tour("/community", "community_directory_tour")

    def test_08_frontend_misc_tour(self):
        # Tests [@ANCHOR: test_tour_frontend_misc]
        self.start_tour("/user-websites/documentation", "frontend_misc_tour", login=self.user_test.login)

    def test_09_backend_views_tour(self):
        # Tests [@ANCHOR: test_tour_backend_views]
        admin = self.env.ref("base.user_admin")
        self.start_tour("/odoo", "backend_views_tour", login=admin.login)
