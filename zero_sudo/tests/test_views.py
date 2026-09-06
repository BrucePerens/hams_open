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
        self.env.ref('base.user_admin').lang = "en_US"

    def test_01_res_users_views(self):
        # [@ANCHOR: zero_sudo:COMM_test_res_users_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_test_res_users_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_setup]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_start_hams_browser]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_teardown]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_teardown_class]
        # (all four run automatically via setUp()/tearDown()/
        # tearDownClass() for every test in this HamsHttpCase-based
        # class, this one included)
        """
        Verify that the zero_sudo res.users views compile and render correctly.
        """
        # Execute get_view to satisfy the AST linter for xpath injections
        self.env["res.users"].get_view(view_type="form")
        self.env["res.users"].get_view(view_type="list")
        self.env["res.users"].get_view(view_type="search")

    def test_02_zero_sudo_tour(self):
        # Tests [@ANCHOR: zero_sudo:hams_http_case_start_tour]

        # Tests [@ANCHOR: zero_sudo:hams_http_case_browser_js]
        # (start_tour() delegates to core Odoo's own HttpCase.start_tour,
        # which calls self.browser_js() -- this class's own override --
        # to actually run the tour JS)
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_handle_request_paused]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_preexec]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_spawn_chrome]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_process_request_thread]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_save_test_file]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_opener_init]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_chrome_init]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_chrome_stop]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_wait_ready]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_chrome_start]
        # (this is a real, end-to-end headless-Chrome tour run -- every
        # one of these monkeypatches on ChromeBrowser/werkzeug internals
        # is on the real, live path a tour actually exercises: spawning
        # Chrome under its own process group, waiting for the CDP socket
        # to come up, driving navigation/requests through the DevTools
        # protocol, and (on failure) saving a screenshot)
        # [@ANCHOR: zero_sudo:COMM_test_zero_sudo_tour]
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
        # Tests [@ANCHOR: zero_sudo:COMM_test_security_log_views]
        """Verify that the security.log views compile and render correctly."""
        self.env["zero_sudo.security.log"].get_view(view_type="form")
        self.env["zero_sudo.security.log"].get_view(view_type="list")
