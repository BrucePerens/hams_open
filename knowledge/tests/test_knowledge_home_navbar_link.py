# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestKnowledgeHomeNavbarLink(HamsHttpCase):
    def setUp(self):
        super().setUp()
        # A visitor who is not signed in sees the manual and the site menu only when sign-up is open
        # (with ham_init installed, an invite-only site redirects them to sign in).
        self.env["ir.config_parameter"].set_param("auth_signup.invitation_scope", "b2c")

    def test_01_site_nav_links_to_knowledge_home(self):
        # Found live 2026-08-29 across three separate hams_com usability-audit
        # personas: /knowledge/home is a real, working manual index, but
        # nothing on the site linked to it, and the site's own search never
        # indexes article content either -- a newcomer with no prior context
        # had no way to discover it. Same bug shape as the hams_com nav-link
        # gaps fixed the same night (QSL Designer, Logbook Anomalies,
        # Propagation Maps, Club Apply).
        response = self.url_open("/knowledge/home")
        self.assertEqual(response.status_code, 200)
        self.assertIn(b'href="/knowledge/home"', response.content)
