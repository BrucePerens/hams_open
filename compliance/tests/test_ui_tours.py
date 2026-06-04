# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

@tagged("post_install", "-at_install")
class TestComplianceUITour(HamsHttpCase):
    def test_compliance_tour(self):
        """Run the compliance tour to verify cookie bar and legal pages."""
        # Tests [@ANCHOR: compliance_footer_links]
        # Tests [@ANCHOR: story_cookie_consent]
        # Tests [@ANCHOR: story_automatic_legal_pages]
        # Tests [@ANCHOR: journey_compliance_setup]
        # Tests [@ANCHOR: compliance_post_init_cookie_bar]
        # Tests [@ANCHOR: compliance_privacy_policy_template]
        # Tests [@ANCHOR: compliance_cookie_policy_template]
        # Tests [@ANCHOR: compliance_terms_of_service_template]
        self.start_tour("/privacy?debug=1", "compliance_tour")
