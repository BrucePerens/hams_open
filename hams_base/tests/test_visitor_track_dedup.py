# -*- coding: utf-8 -*-
from datetime import datetime, timedelta

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestVisitorTrackDedup(HamsHttpCase):
    """Tests [@ANCHOR: hams_base:visitor_track_dedup] (hams1 readiness audit row 13)."""

    def _tracks(self, path):
        return self.env["website.track"].sudo().search([("url", "like", path)])


    def test_repeated_requests_on_one_session_write_the_visitor_once(self):
        from unittest.mock import patch
        import odoo.sql_db as sql_db
        calls = []
        original = sql_db.Cursor.execute

        def counting(cursor, query, *args, **kwargs):
            if "INSERT INTO website_visitor" in str(getattr(query, "code", query)):
                calls.append(1)
            return original(cursor, query, *args, **kwargs)

        with patch.object(sql_db.Cursor, "execute", counting):
            for _ in range(6):
                self.assertEqual(self.url_open("/event").status_code, 200)
        self.assertEqual(len(calls), 1, "a tracked page viewed again in the same session must not UPSERT the visitor again")
        self.assertEqual(len(self._tracks("/event")), 1)

    def test_a_different_page_in_the_same_session_is_still_tracked(self):
        self.url_open("/event")
        self.url_open("/event?search=net")
        self.assertEqual(len(self._tracks("/event")), 2)

    def test_the_page_is_tracked_again_after_the_window(self):
        self.url_open("/event")
        track = self._tracks("/event")
        self.assertEqual(len(track), 1)
        track.write({"visit_datetime": datetime.utcnow() - timedelta(minutes=45)})
        self.url_open("/event")
        self.assertEqual(len(self._tracks("/event")), 2)

    def test_a_new_session_is_a_new_visitor_and_is_tracked(self):
        self.url_open("/event")
        self.opener.cookies.clear()
        self.url_open("/event")
        self.assertEqual(len(self._tracks("/event")), 2)
