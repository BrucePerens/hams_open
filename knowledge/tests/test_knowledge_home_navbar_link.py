# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestKnowledgeHomeNavbarLink(HamsHttpCase):
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
