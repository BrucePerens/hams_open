# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.tests.common import tagged
from .common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestZeroSudoViews(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.env.user.lang = "en_US"

    def test_01_res_users_views(self):
        # [@ANCHOR: zero_sudo:COMM_test_res_users_views]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_res_users_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_test_res_users_views]
        """
        Verify that the zero_sudo res.users views compile and render correctly.
        """
        # Execute get_view to satisfy the AST linter for xpath injections
        self.env["res.users"].get_view(view_type="form")
        self.env["res.users"].get_view(view_type="search")

    def test_02_zero_sudo_tour(self):
        # [@ANCHOR: zero_sudo:COMM_test_zero_sudo_tour]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_zero_sudo_tour]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_login_blocking]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_zero_sudo_tour]
        """Run the zero_sudo_tour to verify UI functionality."""
        # Enforcing ADR-0081 Section 8: Explicitly set ?debug=1 to prevent Owl dev mode crashes
        self.start_tour("/odoo?debug=1", "zero_sudo_tour", login="admin")

    def test_03_noisy_table_views(self):
        # [@ANCHOR: zero_sudo:COMM_test_noisy_table_views]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_noisy_table_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_test_noisy_table_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_UX_NOISY_TABLE_MANAGEMENT]
        """
        Verify that the noisy_table views compile and render correctly.
        """
        self.env["zero_sudo.noisy_table"].get_view(view_type="form")
        self.env["zero_sudo.noisy_table"].get_view(view_type="list")

    def test_04_security_log_views(self):
        # [@ANCHOR: zero_sudo:COMM_test_security_log_views]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_security_log_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_test_security_log_views]
        """Verify that the security.log views compile and render correctly."""
        self.env["zero_sudo.security.log"].get_view(view_type="form")
        self.env["zero_sudo.security.log"].get_view(view_type="list")
