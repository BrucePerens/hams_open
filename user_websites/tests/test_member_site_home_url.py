# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""A member site's home is /<slug>, and /<slug>/home is a permanent redirect to it.

Bruce, 2026-10-12: "make all of the user websites be hams.com/<callsign> without 'home'."
"""
import importlib.util
import os

from odoo.exceptions import ValidationError
from odoo.tests import tagged
from odoo.modules.module import get_module_path
from odoo.tools import mute_logger

from odoo.addons.edge_routing import utils as edge_utils
from odoo.addons.edge_routing.utils import RESERVED_SLUGS, reserved_slug_words, routed_first_segments
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestMemberSiteHomeUrl(HamsHttpCase):
    # [@ANCHOR: test_member_site_home_url]

    def setUp(self):
        super().setUp()
        groups = [
            self.env.ref("base.group_portal").id,
            self.env.ref("user_websites.group_user_websites_user").id,
        ]
        self.owner = self.env["res.users"].create(
            {
                "name": "Home Url Owner",
                "login": "homeurlowner",
                "email": "homeurlowner@example.com",
                "website_slug": "homeurlowner",
                "group_ids": [(6, 0, groups)],
            }
        )
        self.home_page = self.env["website.page"].create(
            {
                "url": "/homeurlowner",
                "name": "Home Url Owner Home",
                "type": "qweb",
                "owner_user_id": self.owner.id,
                "website_published": True,
                "arch": '<t name="H" t-name="user_websites.home_homeurlowner">'
                '<t t-call="user_websites.template_default_home">'
                '<div id="wrap" class="oe_structure"><p>Member home body</p></div>'
                "</t></t>",
            }
        )
        self.env.flush_all()

    def test_01_home_served_at_slug(self):
        # Tests [@ANCHOR: user_websites:fn_ir_http_get_converters]
        # Tests [@ANCHOR: user_websites:COMM_user_home_fallback]
        response = self.url_open("/homeurlowner", allow_redirects=False)
        self.assertEqual(response.status_code, 200)
        self.assertIn(b"Member home body", response.content)

    def test_02_legacy_home_is_permanent_redirect(self):
        # Tests [@ANCHOR: user_websites:COMM_user_home_legacy_redirect]
        response = self.url_open("/homeurlowner/home", allow_redirects=False)
        self.assertEqual(response.status_code, 301)
        self.assertEqual(response.headers["Location"].replace(self.base_url(), ""), "/homeurlowner")
        followed = self.url_open("/homeurlowner/home")
        self.assertEqual(followed.status_code, 200)
        self.assertIn(b"Member home body", followed.content)

    def test_03_legacy_redirect_has_no_open_redirect_or_loop(self):
        # Tests [@ANCHOR: user_websites:COMM_user_home_legacy_redirect]
        for evil in ("//evil.example", "..%2f..%2fevil.example", "%2f%2fevil.example", "https:%2f%2fevil.example"):
            response = self.url_open(f"/{evil}/home", allow_redirects=False)
            location = response.headers.get("Location", "")
            if location:
                self.assertTrue(location.startswith("/"), (evil, location))
                self.assertFalse(location.startswith("//"), (evil, location))
        # The redirect target is a page, never another redirect.
        target = self.url_open("/homeurlowner", allow_redirects=False)
        self.assertNotIn(target.status_code, (301, 302, 303, 307, 308))

    def test_04_uppercase_slug_redirects_to_canonical(self):
        # Tests [@ANCHOR: user_websites:COMM_user_home_fallback]
        response = self.url_open("/HOMEURLOWNER", allow_redirects=False)
        self.assertEqual(response.status_code, 301)
        self.assertEqual(response.headers["Location"].replace(self.base_url(), ""), "/homeurlowner")

    def test_05_non_member_single_segment_paths_still_served(self):
        # Tests [@ANCHOR: user_websites:COMM_user_home_fallback]
        # The /<slug> route sees every one-segment path; stock pages must still fall through.
        self.assertEqual(self.url_open("/contactus").status_code, 200)
        self.assertEqual(self.url_open("/no-such-member-site").status_code, 404)
        self.assertEqual(self.url_open("/no-such-member-site/home").status_code, 404)

    def test_06_rewrite_redirects_still_fire(self):
        # Tests [@ANCHOR: user_websites:COMM_user_home_fallback]
        website = self.env["website"].search([], limit=1)
        self.env["website.rewrite"].create(
            {
                "name": "old member page",
                "website_id": website.id,
                "url_from": "/old-member-page",
                "url_to": "/homeurlowner",
                "redirect_type": "301",
            }
        )
        self.env.flush_all()
        response = self.url_open("/old-member-page", allow_redirects=False)
        self.assertEqual(response.status_code, 301)

    def test_07_navbar_and_directory_link_to_slug(self):
        # Tests [@ANCHOR: user_websites:COMM_user_blog_index]
        html = self.url_open("/homeurlowner/blog").text
        self.assertIn('href="/homeurlowner"', html)
        self.assertNotIn("/homeurlowner/home", html)

    def test_08_reserved_words_cannot_be_slugs(self):
        # Tests [@ANCHOR: user_websites:COMM_res_users_check_reserved_slugs]
        for word in ("home", "admin", "blog", "my", "privacy", "manual", "api"):
            self.assertIn(word, RESERVED_SLUGS)
            with self.assertRaises(ValidationError), mute_logger("odoo.sql_db"):
                self.owner.write({"website_slug": word})
                self.env.flush_all()

    @staticmethod
    def _is_open_source_route(rule, open_source_root):
        """False only for a route that belongs to an addon outside this repository's addons directory."""
        endpoint = getattr(rule.endpoint, "method", rule.endpoint)
        parts = getattr(endpoint, "__module__", "").split(".")
        if parts[:2] != ["odoo", "addons"] or len(parts) < 3:
            return True
        addon_path = get_module_path(parts[2])
        return not addon_path or os.path.dirname(addon_path) == open_source_root

    def test_09_reserved_slugs_cover_static_single_segment_routes(self):
        # Tests [@ANCHOR: user_websites:COMM_res_users_check_reserved_slugs]
        # A static route beats the /<slug> converter route, so a member who claimed one of these
        # words would own a site nobody could reach.
        # Only routes of the open-source modules: a hams.com module installed in the same database adds
        # routes of its own, which the live routing map (reserved_slug_words) reserves without the list.
        open_source_root = os.path.dirname(os.path.dirname(get_module_path("user_websites")))
        router = self.env["ir.http"].routing_map()
        static_first_segments = set()
        for rule in router.iter_rules():
            if not self._is_open_source_route(rule, open_source_root):
                continue
            parts = [p for p in rule.rule.split("/") if p]
            if parts and "<" not in parts[0] and parts[0] != "static":
                static_first_segments.add(parts[0].lower())
        missing = sorted(
            w for w in static_first_segments
            if w.replace("-", "").replace("_", "").isalnum()
            and w not in RESERVED_SLUGS
            and "_" not in w
        )
        self.assertEqual(missing, [], "Add these first path segments to edge_routing.utils.RESERVED_SLUGS")

    def test_09b_routed_first_segments_are_reserved_without_the_fixed_list(self):
        # Tests [@ANCHOR: edge_routing:routed_first_segments]
        # Tests [@ANCHOR: edge_routing:reserved_slug_words]
        # A route added after the fixed list was written still cannot be shadowed: with the fixed list
        # emptied, the live routing map alone must refuse the words of core and platform routes.
        self.assertIn("web", routed_first_segments(self.env))
        self.safe_patch_object(edge_utils, "RESERVED_SLUGS", frozenset())
        for word in ("web", "odoo"):
            self.assertIn(word, reserved_slug_words(self.env))
            with self.assertRaises(ValidationError), mute_logger("odoo.sql_db"):
                self.owner.write({"website_slug": word})
                self.env.flush_all()
            with self.assertRaises(ValidationError), mute_logger("odoo.sql_db"):
                self.env["user.websites.group"].create({"name": "Shadow " + word, "website_slug": word})
                self.env.flush_all()
        # A name that slugifies to a routed word gets another slug, not a refusal.
        self.assertNotIn(self.env["res.users"]._generate_unique_slug("Web"), ("web", ""))
        self.assertNotIn("homeurlowner", reserved_slug_words(self.env))

    def test_09c_migration_reports_reserved_slug_collisions(self):
        # [@ANCHOR: test_migration_report_reserved_slug_collisions]
        # Tests [@ANCHOR: user_websites:migrate_report_reserved_slug_collisions]
        self.env.cr.execute("UPDATE res_users SET website_slug = 'calendar' WHERE id = %s", (self.owner.id,))
        path = os.path.join(os.path.dirname(__file__), "..", "migrations", "0.6", "post-migration.py")
        spec = importlib.util.spec_from_file_location("uw_migrate_06", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with self.assertLogs("uw_migrate_06", level="ERROR"):
            found = module.migrate(self.env.cr, "0.5")
        self.assertIn(("member", self.owner.id, "calendar"), found)
        self.env.cr.execute("SELECT website_slug FROM res_users WHERE id = %s", (self.owner.id,))
        self.assertEqual(self.env.cr.fetchone()[0], "calendar")  # reports only; the data is not changed

    def test_10_migration_moves_home_pages_to_slug(self):
        # Tests [@ANCHOR: user_websites:migrate_home_url_to_slug]
        # [@ANCHOR: test_migration_home_url_to_slug]
        self.home_page.write({"url": "/homeurlowner/home"})
        self.env.flush_all()
        path = os.path.join(
            os.path.dirname(__file__), "..", "migrations", "0.5", "post-migration.py"
        )
        spec = importlib.util.spec_from_file_location("uw_migrate_05", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.migrate(self.env.cr, "0.4")
        module.migrate(self.env.cr, "0.4")  # idempotent
        self.home_page.invalidate_recordset(["url"])
        self.assertEqual(self.home_page.url, "/homeurlowner")

    def test_11_language_prefixed_paths_still_work(self):
        # Tests [@ANCHOR: user_websites:fn_site_slug_converter_to_python]
        # [@ANCHOR: test_language_prefixed_paths_still_work]
        # Tests [@ANCHOR: user_websites:site_slug_converter]
        # The /<slug> routes must not shadow Odoo's /<lang>/... handling (found 2026-10-13: /en_US/
        # redirected to /en_US, which answered 404).
        response = self.url_open("/en_US/", allow_redirects=False)
        self.assertNotEqual(response.status_code, 404)
        followed = self.url_open("/en_US/")
        self.assertEqual(followed.status_code, 200)
        self.assertIn(b"</html>", followed.content)
        self.assertNotEqual(self.url_open("/en_US", allow_redirects=False).status_code, 404)
        self.assertEqual(self.url_open("/en_US/blog").status_code, 200)
        # A member site is untouched.
        self.assertEqual(self.url_open("/homeurlowner").status_code, 200)

    def test_12_second_language_prefix_is_not_a_site(self):
        # Tests [@ANCHOR: user_websites:fn_site_slug_converter_to_python]
        # Tests [@ANCHOR: user_websites:site_slug_converter]
        french = self.env["res.lang"].with_context(active_test=False).search([("code", "=", "fr_FR")])
        french.active = True
        website = self.env["website"].search([], limit=1)
        website.language_ids = [(4, french.id)]
        self.env.flush_all()
        self.env.registry.clear_cache()
        for path in ("/fr", "/fr/", "/fr/blog", "/fr/contactus"):
            response = self.url_open(path, allow_redirects=False)
            self.assertNotEqual(response.status_code, 404, path)

    def test_13_language_codes_cannot_be_slugs(self):
        # Tests [@ANCHOR: edge_routing:language_slugs]
        for word in ("en",):
            with self.assertRaises(ValidationError), mute_logger("odoo.sql_db"):
                self.owner.write({"website_slug": word})
                self.env.flush_all()
