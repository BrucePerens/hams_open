# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The Help pages show member user manuals only.

Internal documentation (stories, journeys, External Dependencies, module
reference) must not be listed, searched or reachable by URL for anyone but a
Manual Administrator, and even an administrator meets it only in the labelled
administrators-only view, never in Help itself.
"""
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

NEEDLE = "Zorkmidquartz"


@tagged("post_install", "-at_install")
class TestHelpAudience(HamsHttpCase):
    def setUp(self):
        super().setUp()
        # hams.com's invite-only gate (ham_init) sends signed-out visitors to
        # the sign-in page; these tests are about what Help shows, so open
        # sign-up the way the public site is when anyone can join.
        self.env["ir.config_parameter"].set_param(
            "auth_signup.invitation_scope", "b2c"
        )
        Article = self.env["knowledge.article"]
        self.manual = Article.create(
            {
                "name": "Zorkmid Relay Setup Guide",
                "body": f"<p>How to set up your relay. {NEEDLE}</p>",
                "is_published": True,
                "internal_permission": "read",
                "audience": "member",
                "help_topic": "Operating your station",
            }
        )
        self.story = Article.create(
            {
                "name": "Story: Zorkmid Ticket Lifecycle",
                "body": f"<p>A developer story. {NEEDLE}</p>",
                "is_published": False,
                "internal_permission": "none",
                "audience": "internal",
            }
        )
        # Published and readable by the record rules, but internal: only the
        # controller's audience check keeps it out of Help.
        self.published_internal = Article.create(
            {
                "name": "External Dependencies Zorkmid",
                "body": f"<p>Third party packages. {NEEDLE}</p>",
                "is_published": True,
                "internal_permission": "read",
                "audience": "internal",
            }
        )
        groups = self.env.ref
        self.portal_user = self.env["res.users"].create(
            {
                "name": "Zorkmid Member",
                "login": "zorkmid_member",
                "password": "zorkmid_member",
                "group_ids": [(6, 0, [groups("base.group_portal").id])],
            }
        )
        self.staff_user = self.env["res.users"].create(
            {
                "name": "Zorkmid Staff",
                "login": "zorkmid_staff",
                "password": "zorkmid_staff",
                "group_ids": [(6, 0, [groups("base.group_user").id])],
            }
        )
        self.manager_user = self.env["res.users"].create(
            {
                "name": "Zorkmid Manager",
                "login": "zorkmid_manager",
                "password": "zorkmid_manager",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            groups("base.group_user").id,
                            groups("knowledge.group_manual_manager").id,
                        ],
                    )
                ],
            }
        )

    def _as(self, login):
        if login:
            self.authenticate(login, login)
        else:
            self.authenticate(None, None)

    def _internal_names(self):
        return [
            self.story.name.encode(),
            self.published_internal.name.encode(),
        ]

    def test_01_help_index_lists_only_member_manuals(self):
        # Tests [@ANCHOR: knowledge:help_index]
        # Tests [@ANCHOR: knowledge:help_audience_domain]
        # Tests [@ANCHOR: knowledge:help_topics]
        # Tests [@ANCHOR: knowledge:sidebar_context]
        for login in (None, "zorkmid_member", "zorkmid_staff", "zorkmid_manager"):
            self._as(login)
            for url in ("/manual", "/knowledge/home"):
                response = self.url_open(url)
                self.assertEqual(response.status_code, 200, (login, url))
                self.assertIn(b"Zorkmid Relay Setup Guide", response.content)
                self.assertIn(b"Operating your station", response.content)
                for name in self._internal_names():
                    self.assertNotIn(name, response.content, (login, url))

    def test_02_index_is_not_whichever_article_installed_first(self):
        self._as(None)
        response = self.url_open("/manual")
        self.assertEqual(response.url.rstrip("/").split("/")[-1], "manual")
        self.assertIn(b"o_help_topic", response.content)

    def test_03_search_finds_only_member_manuals(self):
        # Tests [@ANCHOR: knowledge:help_search_member_only]
        for login in (None, "zorkmid_member", "zorkmid_manager"):
            self._as(login)
            response = self.url_open(f"/manual/search?search={NEEDLE}")
            self.assertEqual(response.status_code, 200)
            self.assertIn(b"Zorkmid Relay Setup Guide", response.content)
            for name in self._internal_names():
                self.assertNotIn(name, response.content, login)

    def test_04_internal_article_url_is_404_for_non_administrators(self):
        # Tests [@ANCHOR: knowledge:internal_article_url_404]
        # Tests [@ANCHOR: knowledge:can_open_article]
        for login in (None, "zorkmid_member", "zorkmid_staff"):
            self._as(login)
            for article in (self.story, self.published_internal):
                response = self.url_open(article.website_url)
                self.assertEqual(response.status_code, 404, (login, article.name))

    def test_05_internal_article_by_name_is_404_for_non_administrators(self):
        for login in (None, "zorkmid_member", "zorkmid_staff"):
            self._as(login)
            for name in ("Story:+Zorkmid+Ticket+Lifecycle", "External+Dependencies+Zorkmid"):
                response = self.url_open(f"/manual/by_name/{name}")
                self.assertEqual(response.status_code, 404, (login, name))

    def test_06_member_manual_still_opens_by_url_and_by_name(self):
        for login in (None, "zorkmid_member"):
            self._as(login)
            self.assertEqual(self.url_open(self.manual.website_url).status_code, 200)
            response = self.url_open("/manual/by_name/Zorkmid+Relay+Setup+Guide")
            self.assertEqual(response.status_code, 200)
            self.assertIn(b"Zorkmid Relay Setup Guide", response.content)

    def test_07_administrators_open_internal_articles_with_a_banner(self):
        self._as("zorkmid_manager")
        for article in (self.story, self.published_internal):
            response = self.url_open(article.website_url)
            self.assertEqual(response.status_code, 200, article.name)
            self.assertIn(b"Internal documentation", response.content)
        response = self.url_open("/manual/by_name/External+Dependencies+Zorkmid")
        self.assertEqual(response.status_code, 200)
        response = self.url_open(self.manual.website_url)
        self.assertNotIn(b"it is not part of the Help", response.content)

    def test_08_internal_index_is_for_administrators_only(self):
        # Tests [@ANCHOR: knowledge:internal_index]
        # Tests [@ANCHOR: knowledge:help_internal_domain]
        # Tests [@ANCHOR: knowledge:is_manual_manager]
        for login in (None, "zorkmid_member", "zorkmid_staff"):
            self._as(login)
            response = self.url_open("/manual/internal")
            self.assertEqual(response.status_code, 404, login)
        self._as("zorkmid_manager")
        response = self.url_open("/manual/internal")
        self.assertEqual(response.status_code, 200)
        self.assertIn(b"administrators only", response.content)
        for name in self._internal_names():
            self.assertIn(name, response.content)
        self.assertNotIn(b"Zorkmid Relay Setup Guide", response.content)
        response = self.url_open("/manual/internal?search=Lifecycle")
        self.assertIn(self.story.name.encode(), response.content)
        self.assertNotIn(self.published_internal.name.encode(), response.content)

    def test_09_help_links_to_the_internal_view_only_for_administrators(self):
        self._as("zorkmid_member")
        self.assertNotIn(b"/manual/internal", self.url_open("/manual").content)
        self._as("zorkmid_manager")
        self.assertIn(b"/manual/internal", self.url_open("/manual").content)

    def test_10_site_search_never_returns_internal_articles(self):
        # Tests [@ANCHOR: knowledge:COMM_search_get_detail]
        website = self.env["website"].search([], limit=1)
        for user in (self.portal_user, self.manager_user):
            Article = self.env["knowledge.article"].with_user(user)
            detail = Article._search_get_detail(
                website, "name asc", {"displayDescription": True}
            )
            found = Article.search(detail["base_domain"][0] + detail["base_domain"][1])
            self.assertIn(self.manual, found, user.login)
            self.assertNotIn(self.story, found, user.login)
            self.assertNotIn(self.published_internal, found, user.login)

    def test_11_audience_defaults_member_for_hand_written_articles(self):
        # An administrator's own published article keeps working; the
        # documentation loader always sets the audience explicitly.
        # Tests [@ANCHOR: knowledge:article_audience]
        article = self.env["knowledge.article"].create({"name": "Hand Written"})
        self.assertEqual(article.audience, "member")

    def test_12_a_lower_sequence_leads_its_help_heading(self):
        # Tests [@ANCHOR: knowledge:help_topics]
        from odoo.addons.knowledge.controllers.main import ManualLibraryController

        Article = self.env["knowledge.article"]
        common = {"body": "<p>x</p>", "is_published": True, "internal_permission": "read", "audience": "member", "help_topic": "Getting started and your account"}
        late = Article.create(dict(common, name="Aardvark Zorkmid Guide"))
        lead = Article.create(dict(common, name="Zulu Zorkmid Lead", sequence=1))
        topics = dict(ManualLibraryController()._help_topics(late | lead))
        self.assertEqual([a.name for a in topics["Getting started and your account"]], ["Zulu Zorkmid Lead", "Aardvark Zorkmid Guide"])
