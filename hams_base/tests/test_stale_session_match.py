# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import odoo.http
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestStaleSessionBeforeWebsiteMatch(HamsHttpCase):
    """A session whose user no longer exists must be served as the public user.

    Reproduces the hams.com production 403 of 2026-09-23/24
    ("Access Denied by ACLs for operation: read, uid: 99, model: website" on
    ``GET /``), where ``uid: 99`` had no ``res_users`` row.
    """

    def setUp(self):
        super().setUp()
        self.user = self.env["res.users"].create(
            {
                "name": "Stale Session Tester",
                "login": "stale_session_tester",
                "password": "stale_session_tester",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def _point_session_at_deleted_user(self):
        """Authenticate for real, then make the stored session carry a uid with
        no ``res_users`` row, which is what a session cookie of a since-deleted
        account looks like on disk."""
        session = self.authenticate("stale_session_tester", "stale_session_tester")
        last_user = (
            self.env["res.users"]
            .with_context(active_test=False)
            .search([], order="id desc", limit=1)
        )
        missing_uid = last_user.id + 1000
        self.assertFalse(
            self.env["res.users"].browse(missing_uid).exists(),
            "[!] DIAGNOSTIC FOR AI: the chosen uid must not exist for this test to mean anything.",
        )
        session.uid = missing_uid
        odoo.http.root.session_store.save(session)
        return session, missing_uid

    def test_homepage_with_deleted_user_session_is_public(self):
        # Tests [@ANCHOR: hams_base_validate_session_before_website_match]
        session, missing_uid = self._point_session_at_deleted_user()
        # Production failed right after "Invalidating caches after database
        # signaling": with warm caches the pre-authentication reads are served
        # from ormcache and never reach the ACL check. Start cold, as there.
        self.env.registry.clear_cache()

        response = self.url_open("/", allow_redirects=False)
        self.assertNotEqual(
            response.status_code,
            403,
            "[!] DIAGNOSTIC FOR AI: GET / returned 403 for a session whose uid "
            f"({missing_uid}) has no res_users row. ir.http._match is reading "
            "website records under the unvalidated session uid; see "
            "hams_base/models/ir_http.py.",
        )
        self.assertLess(response.status_code, 400)

        stored = odoo.http.root.session_store.get(session.sid)
        self.assertIsNone(
            stored.uid,
            "[!] DIAGNOSTIC FOR AI: the dead session must be logged out so later "
            "requests from the same browser are anonymous.",
        )

    def test_valid_session_is_untouched(self):
        # Tests [@ANCHOR: hams_base_validate_session_before_website_match]
        session = self.authenticate("stale_session_tester", "stale_session_tester")
        self.env.registry.clear_cache()

        response = self.url_open("/my/home")
        self.assertLess(
            response.status_code,
            400,
            "[!] DIAGNOSTIC FOR AI: a valid portal session must still reach /my/home.",
        )
        stored = odoo.http.root.session_store.get(session.sid)
        self.assertEqual(stored.uid, self.user.id)
