# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General
# Public License v3.0 (AGPL-3.0).
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestComplianceUITour(HamsHttpCase):
    def test_compliance_tour(self):
        # [@ANCHOR: COMM_test_compliance_ui_tour]
        """Run the compliance tour to verify cookie bar and legal pages."""
        # Tests [@ANCHOR: COMM_compliance_footer_links]

        # Tests [@ANCHOR: COMM_story_cookie_consent]

        # Tests [@ANCHOR: COMM_story_automatic_legal_pages]

        # Tests [@ANCHOR: COMM_journey_compliance_setup]

        # Tests [@ANCHOR: COMM_compliance_post_init_cookie_bar]

        # Tests [@ANCHOR: COMM_compliance_privacy_policy_template]

        # Tests [@ANCHOR: COMM_compliance_cookie_policy_template]

        # Tests [@ANCHOR: COMM_compliance_terms_of_service_template]

        # Tests [@ANCHOR: COMM_compliance_accessibility_statement_template]

        self.url_open("/privacy")
        self.start_tour("/en_US/privacy?debug=1", "compliance_tour")
