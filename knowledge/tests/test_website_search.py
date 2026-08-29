# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestKnowledgeArticleWebsiteSearch(HamsHttpCase):
    def setUp(self):
        super().setUp()
        self.published_article = self.env["knowledge.article"].create(
            {
                "name": "Xyzzyplugh Interfacing Guide",
                "body": "<p>Wire a Xyzzyplugh cable to your radio's data port.</p>",
                "is_published": True,
                "internal_permission": "read",
            }
        )
        self.unpublished_article = self.env["knowledge.article"].create(
            {
                "name": "Xyzzyplugh Internal Notes",
                "body": "<p>Xyzzyplugh vendor pricing, not for public eyes.</p>",
                "is_published": False,
                "internal_permission": "read",
            }
        )
        self.member_user = self.env["res.users"].create(
            {
                "name": "Xyzzyplugh Member",
                "login": "xyzzyplugh_member",
                "password": "xyzzyplugh_member",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.member_only_article = self.env["knowledge.article"].create(
            {
                "name": "Xyzzyplugh Team Onboarding",
                "body": "<p>Xyzzyplugh internal team-only onboarding notes.</p>",
                "is_published": False,
                "internal_permission": "none",
                "member_ids": [(4, self.member_user.id)],
            }
        )
        self.internal_user = self.env["res.users"].create(
            {
                "name": "Xyzzyplugh Staff",
                "login": "xyzzyplugh_staff",
                "password": "xyzzyplugh_staff",
            }
        )

    def test_01_search_finds_a_published_article_by_title(self):
        # Found live 2026-08-29 across three separate hams_com usability-audit
        # personas: the site's own search never covered knowledge articles at
        # all, only products/blog/pages -- a genuinely public, published
        # manual article was completely unfindable through search. This
        # verifies the fix (knowledge.article now registers itself with
        # website's search dispatch) actually works end to end through the
        # real /website/search route, not just at the model level.
        response = self.url_open("/website/search?search=Xyzzyplugh")
        self.assertEqual(response.status_code, 200)
        # Not a literal "Xyzzyplugh Interfacing Guide" substring check: the
        # matched search term is wrapped in its own highlight span by
        # website's own autocomplete() rendering, splitting the title's raw
        # text across tags. Check the two halves separately instead.
        self.assertIn(b"Xyzzyplugh", response.content)
        self.assertIn(b"Interfacing Guide", response.content)

    def test_02_search_does_not_leak_an_unpublished_article(self):
        response = self.url_open("/website/search?search=Xyzzyplugh")
        self.assertEqual(response.status_code, 200)
        self.assertNotIn(b"Xyzzyplugh Internal Notes", response.content)

    def test_03_a_member_can_find_their_own_member_only_article(self):
        # Found reviewing this same-night fix: the first version of this
        # domain checked is_published only, so a logged-in member searching
        # for an article they're specifically a member of -- an article
        # already directly visible to them everywhere else on the site --
        # would have gotten zero results here. Matches the same visibility
        # rule controllers/main.py's own listing endpoints already use.
        self.authenticate(self.member_user.login, self.member_user.login)
        response = self.url_open("/website/search?search=Xyzzyplugh")
        self.assertEqual(response.status_code, 200)
        self.assertIn(b"Team Onboarding", response.content)

    def test_04_a_non_member_does_not_see_a_member_only_article(self):
        response = self.url_open("/website/search?search=Xyzzyplugh")
        self.assertEqual(response.status_code, 200)
        self.assertNotIn(b"Team Onboarding", response.content)

    def test_05_internal_staff_can_find_any_unpublished_article(self):
        self.authenticate(self.internal_user.login, self.internal_user.login)
        response = self.url_open("/website/search?search=Xyzzyplugh")
        self.assertEqual(response.status_code, 200)
        self.assertIn(b"Internal Notes", response.content)
