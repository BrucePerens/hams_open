# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

PAGE = "/hams-track-dedup-probe"


@tagged("post_install", "-at_install")
class TestVisitorTrackDedup(HamsHttpCase):
    """Tests [@ANCHOR: hams_base:visitor_track_dedup] (hams1 readiness audit row 13).

    The probe is a tracked website page (core's own `_register_website_track` path, the same one /event and /blog use)."""

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.env["website.page"].create({
            "name": "Track dedup probe",
            "url": PAGE,
            "type": "qweb",
            "track": True,
            "website_published": True,
            "arch": '<t t-call="website.layout"><div id="wrap"><p>probe</p></div></t>',
        })

    def _count(self):
        self.env.cr.execute("SELECT count(*) FROM website_track WHERE url LIKE %s", [f"%{PAGE}%"])
        return self.env.cr.fetchone()[0]

    def test_the_probe_page_is_tracked_once_and_served(self):
        self.assertEqual(self.url_open(PAGE).status_code, 200)
        self.assertEqual(self._count(), 1)

    def test_repeated_requests_on_one_session_add_no_more_tracks(self):
        for _ in range(6):
            self.assertEqual(self.url_open(PAGE).status_code, 200)
        self.assertEqual(self._count(), 1, "a tracked page viewed again in the same session must not write again")

    def test_a_different_url_in_the_same_session_is_still_tracked(self):
        self.url_open(PAGE)
        self.url_open(PAGE + "?a=1")
        self.assertEqual(self._count(), 2)

    def test_the_page_is_tracked_again_after_the_window(self):
        self.url_open(PAGE)
        self.env.cr.execute(
            "UPDATE website_track SET visit_datetime = visit_datetime - interval '45 minutes' WHERE url LIKE %s",
            [f"%{PAGE}%"],
        )
        self.url_open(PAGE)
        self.assertEqual(self._count(), 2)

    def test_a_new_session_is_a_new_visitor_and_is_tracked(self):
        self.url_open(PAGE)
        self.opener.cookies.clear()
        self.url_open(PAGE)
        self.assertEqual(self._count(), 2)
