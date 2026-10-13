# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""One site per member: a slug rename re-homes the site, a retired slug redirects, the policy hook.

Bruce, 2026-10-13: "hams1 publishes two web sites for the user. One is bruce-perens-k6bp, and one
is k6bp" and "The 'edit' and 'unpublish' button on bruce-perens-k6bp get a 404".
"""
from unittest.mock import patch

import odoo.http
from odoo.exceptions import ValidationError
from odoo.modules.module import load_script
from odoo.tests import tagged

from odoo.addons.user_websites.models.res_users import ResUsers
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

ARCH = (
    '<t name="H" t-name="user_websites.home_{key}"><t t-call="user_websites.template_default_home">'
    '<div id="wrap" class="oe_structure"><p>{body}</p></div></t></t>'
)


def _page(env, owner, url, body, published=True):
    return env["website.page"].create(
        {
            "url": url,
            "name": body,
            "type": "qweb",
            "owner_user_id": owner.id,
            "website_published": published,
            "arch": ARCH.format(key=url.strip("/").replace("/", "_"), body=body),
        }
    )


@tagged("post_install", "-at_install")
class TestSiteSlugOneSite(HamsHttpCase):
    # [@ANCHOR: test_site_slug_one_site]

    def setUp(self):
        super().setUp()
        groups = [self.env.ref("base.group_portal").id, self.env.ref("user_websites.group_user_websites_user").id]
        self.owner = self.env["res.users"].create(
            {
                "name": "One Site Owner (K6ZZ)",
                "login": "onesiteowner",
                "password": "onesiteowner",
                "email": "onesiteowner@example.com",
                "website_slug": "one-site-owner-k6zz",
                "group_ids": [(6, 0, groups)],
            }
        )
        self.home = _page(self.env, self.owner, "/one-site-owner-k6zz", "old home body")
        self.about = _page(self.env, self.owner, "/one-site-owner-k6zz/about", "about body")
        self.env.flush_all()

    def _rename(self, slug):
        self.owner.write({"website_slug": slug})
        self.env.flush_all()
        self.env.cr.flush()

    def _csrf(self):
        return {"csrf_token": odoo.http.Request.csrf_token(self)}

    def _post(self, path, **data):
        data.update(self._csrf())
        return self.url_open(path, data=data, method="POST", allow_redirects=False)

    def test_01_rename_moves_the_pages_and_the_old_address_redirects(self):
        # Tests [@ANCHOR: user_websites:fn_rehome_site]
        self._rename("k6zz")
        self.home.invalidate_recordset()
        self.assertEqual(self.home.url, "/k6zz")
        self.assertEqual(self.about.url, "/k6zz/about")
        self.assertEqual(self.url_open("/k6zz", allow_redirects=False).status_code, 200)
        old = self.url_open("/one-site-owner-k6zz", allow_redirects=False)
        self.assertEqual(old.status_code, 301)
        self.assertTrue(old.headers["Location"].endswith("/k6zz"))
        sub = self.url_open("/one-site-owner-k6zz/about", allow_redirects=False)
        self.assertEqual(sub.status_code, 301)
        self.assertTrue(sub.headers["Location"].endswith("/k6zz/about"))
        self.assertEqual(self.url_open("/k6zz/about", allow_redirects=False).status_code, 200)

    def test_02_collision_keeps_the_old_page_unpublished_under_the_new_slug(self):
        # The K6BP case: the new address already has a home page.
        new_home = _page(self.env, self.owner, "/k6zz", "new home body")
        self._rename("k6zz")
        self.home.invalidate_recordset()
        new_home.invalidate_recordset()
        self.assertEqual(new_home.url, "/k6zz")
        self.assertEqual(self.home.url, "/k6zz/former-home")
        self.assertFalse(self.home.website_published)
        self.assertTrue(self.home.exists(), "nothing is deleted")
        self.assertIn(b"new home body", self.url_open("/k6zz").content)
        self.assertEqual(self.url_open("/one-site-owner-k6zz", allow_redirects=False).status_code, 301)

    def test_03_toolbar_actions_work_on_a_kept_page(self):
        # The exact production case: the toolbar links /my/site/<current slug>/page/<id> and the page
        # must live under the current slug, or the page-url prefix check answers 404.
        _page(self.env, self.owner, "/k6zz", "new home body")
        self._rename("k6zz")
        self.authenticate("onesiteowner", "onesiteowner")
        self.assertEqual(self.url_open(f"/my/site/k6zz/page/{self.home.id}", allow_redirects=False).status_code, 200)
        done = self._post(f"/my/site/k6zz/page/{self.home.id}/publish", publish="1")
        self.assertIn(done.status_code, (200, 302, 303))
        self.home.invalidate_recordset()
        self.assertTrue(self.home.website_published)

    def test_04_owner_urls_under_the_retired_slug_redirect(self):
        # Tests [@ANCHOR: user_websites:owner_redirect_legacy_slug]
        self._rename("k6zz")
        self.authenticate("onesiteowner", "onesiteowner")
        got = self.url_open("/my/site/one-site-owner-k6zz", allow_redirects=False)
        self.assertEqual(got.status_code, 301)
        self.assertTrue(got.headers["Location"].endswith("/my/site/k6zz"))
        got = self.url_open(f"/my/site/one-site-owner-k6zz/page/{self.about.id}?x=1", allow_redirects=False)
        self.assertEqual(got.status_code, 301)
        self.assertTrue(got.headers["Location"].endswith(f"/my/site/k6zz/page/{self.about.id}?x=1"))
        posted = self._post(f"/my/site/one-site-owner-k6zz/page/{self.about.id}/publish", publish="0")
        self.assertEqual(posted.status_code, 308)
        self.assertTrue(posted.headers["Location"].endswith(f"/my/site/k6zz/page/{self.about.id}/publish"))
        # A slug nobody ever held is still a 404.
        self.assertEqual(self.url_open("/my/site/never-held", allow_redirects=False).status_code, 404)

    def test_05_redirect_chain_is_flat_and_a_reclaimed_slug_is_not_shadowed(self):
        self._rename("k6zz")
        self._rename("k6zz-2")
        rows = self.env["website.rewrite"].search([("url_from", "in", ["/one-site-owner-k6zz", "/k6zz"])])
        self.assertEqual({r.url_from: r.url_to for r in rows}, {"/one-site-owner-k6zz": "/k6zz-2", "/k6zz": "/k6zz-2"})
        self._rename("k6zz")
        self.assertFalse(self.env["website.rewrite"].search([("url_from", "=", "/k6zz")]))
        self.assertEqual(self.url_open("/k6zz", allow_redirects=False).status_code, 200)

    def test_06_migration_folds_pages_left_under_a_retired_slug(self):
        # Tests [@ANCHOR: user_websites:migrate_rehome_stale_site_pages]
        # [@ANCHOR: test_migration_rehome_stale_site_pages]
        self._rename("k6zz")
        # Recreate the pre-0.7 state: pages back at the old address, a page already at the new one.
        self.env.cr.execute("UPDATE website_page SET url = '/one-site-owner-k6zz' WHERE id = %s", (self.home.id,))
        self.env.cr.execute("UPDATE website_page SET url = '/one-site-owner-k6zz/about' WHERE id = %s", (self.about.id,))
        keeper = _page(self.env, self.owner, "/k6zz", "new home body")
        self.env.flush_all()
        migration = load_script("user_websites/migrations/0.7/post-migration.py", "odoo.upgrade.user_websites.0.7.post-migration")
        moved = migration.migrate(self.env.cr, "0.6")
        self.assertEqual(len(moved), 2)
        for rec in (self.home, self.about, keeper):
            rec.invalidate_recordset()
        self.assertEqual(self.about.url, "/k6zz/about")
        self.assertEqual(self.home.url, "/k6zz/former-home")
        self.assertFalse(self.home.website_published)
        self.assertEqual(keeper.url, "/k6zz")
        self.assertEqual(migration.migrate(self.env.cr, "0.6"), [], "idempotent")

    # ------------------------------------------------------------ slug policy

    def _policy(self, value):
        self.env["ir.config_parameter"].set_param("user_websites.slug_policy", value)

    def _callsign_identity(self):
        """Stand-in for hams_com's callsign: the user's job-title field."""
        return (
            patch.object(ResUsers, "_slug_identity_fields", lambda s: ("function",)),
            patch.object(ResUsers, "_slug_identity", lambda s, vals=None: (vals or {}).get("function") or (s.function if s else False)),
        )

    def test_07_open_source_default_slug_follows_the_name(self):
        user = self.env["res.users"].create({"name": "Default Policy (K6DD)", "login": "defpol", "email": "d@example.com"})
        self.assertEqual(user.website_slug, "default-policy-k6dd")

    def test_08_callsign_policy_one_slug_from_the_callsign(self):
        self._policy("callsign")
        p1, p2 = self._callsign_identity()
        with p1, p2:
            user = self.env["res.users"].create(
                {"name": "Pol User (K6PP)", "login": "poluser", "email": "p@example.com", "function": "K6PP"}
            )
            self.assertEqual(user.website_slug, "k6pp")
            # A name change never makes a second slug.
            user.write({"name": "Pol User Renamed"})
            self.assertEqual(user.website_slug, "k6pp")
            # An explicit name-style slug is overridden by the callsign.
            user.write({"website_slug": "pol-user-k6pp"})
            self.assertEqual(user.website_slug, "k6pp")
            # Portable suffix: K6PP/P is k6pp-p.
            other = self.env["res.users"].create(
                {"name": "Portable", "login": "portable", "email": "po@example.com", "function": "K6PP/P"}
            )
            self.assertEqual(other.website_slug, "k6pp-p")
            # Vanity change: new slug, the old one redirects, the pages follow.
            _page(self.env, user, "/k6pp", "pol home")
            user.write({"function": "W6PP"})
            self.assertEqual(user.website_slug, "w6pp")
            self.assertEqual(self.env["website.page"].search([("owner_user_id", "=", user.id)]).url, "/w6pp")
            self.assertTrue(self.env["website.rewrite"].search([("url_from", "=", "/k6pp"), ("url_to", "=", "/w6pp")]))

    def test_09_callsign_policy_no_callsign_no_site_and_collisions_fail(self):
        self._policy("callsign")
        p1, p2 = self._callsign_identity()
        with p1, p2:
            user = self.env["res.users"].create({"name": "No Callsign Yet", "login": "nocall", "email": "n@example.com"})
            self.assertFalse(user.website_slug)
            user.write({"name": "No Callsign Renamed"})
            self.assertFalse(user.website_slug, "no name-derived slug under the callsign policy")
            user.write({"function": "K6NN"})
            self.assertEqual(user.website_slug, "k6nn")
            with self.assertRaises(ValidationError):
                self.env["res.users"].create(
                    {"name": "Dup", "login": "dupcall", "email": "du@example.com", "function": "K6NN"}
                )
