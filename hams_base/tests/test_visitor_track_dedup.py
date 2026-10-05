# -*- coding: utf-8 -*-
from types import SimpleNamespace
from unittest.mock import MagicMock

from odoo.addons.hams_base.models import website_visitor as visitor_module
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

TOKEN = "0123456789abcdef0123456789abcdef"
URL = "http://hams.test/event"


@tagged("post_install", "-at_install")
class TestVisitorTrackDedup(HamsTransactionCase):
    """Tests [@ANCHOR: hams_base:visitor_track_dedup] (hams1 readiness audit row 13).

    The decision is tested directly against real visitor and track rows; only the HTTP request object and the
    core method that would do the writes are replaced."""

    def setUp(self):
        super().setUp()
        self.visitors = self.env["website.visitor"]
        self.visitor = self.visitors.create({"access_token": TOKEN})
        self.core_write = MagicMock(name="core _get_visitor_from_request")
        self.safe_patch_object(type(self.visitors), "_get_visitor_from_request", self.core_write)
        self.safe_patch_object(type(self.visitors), "_get_access_token", lambda model: TOKEN)
        self._request(URL)

    def _request(self, url):
        self.safe_patch_object(visitor_module, "request", SimpleNamespace(httprequest=SimpleNamespace(url=url)))

    def _track(self, url=URL, minutes_ago=0):
        track = self.env["website.track"].create({"visitor_id": self.visitor.id, "url": url})
        if minutes_ago:
            self.env.cr.execute(
                "UPDATE website_track SET visit_datetime = visit_datetime - make_interval(mins => %s) WHERE id = %s",
                [minutes_ago, track.id],
            )
        return track

    def test_a_recent_track_for_the_same_url_skips_every_write(self):
        self._track()
        self.assertTrue(self.visitors._track_exists_for_this_request())
        self.visitors._handle_webpage_dispatch(False)
        self.core_write.assert_not_called()

    def test_no_track_means_core_runs_and_writes(self):
        self.assertFalse(self.visitors._track_exists_for_this_request())
        self.visitors._handle_webpage_dispatch(False)
        self.core_write.assert_called_once()

    def test_a_different_url_is_still_tracked(self):
        self._track(url="http://hams.test/blog")
        self.assertFalse(self.visitors._track_exists_for_this_request())
        self.visitors._handle_webpage_dispatch(False)
        self.core_write.assert_called_once()

    def test_an_old_track_no_longer_suppresses(self):
        self._track(minutes_ago=self.visitors.TRACK_DEDUP_MINUTES + 15)
        self.assertFalse(self.visitors._track_exists_for_this_request())

    def test_a_track_just_inside_the_window_still_suppresses(self):
        self._track(minutes_ago=self.visitors.TRACK_DEDUP_MINUTES - 5)
        self.assertTrue(self.visitors._track_exists_for_this_request())

    def test_another_sessions_track_does_not_suppress(self):
        other = self.visitors.create({"access_token": "f" * 32})
        self.env["website.track"].create({"visitor_id": other.id, "url": URL})
        self.assertFalse(self.visitors._track_exists_for_this_request())

    def test_no_request_context_means_no_suppression(self):
        self.safe_patch_object(type(self.visitors), "_get_access_token", MagicMock(side_effect=ValueError("no request")))
        self.assertFalse(self.visitors._track_exists_for_this_request())
