# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import HamsHttpCase

@tagged("post_install", "-at_install")
class TestZeroSudoViews(HamsHttpCase):

    def test_01_res_users_views(self):
        # [@ANCHOR: test_res_users_views]
        # Tests [@ANCHOR: test_res_users_views]
        """
        Verify that the zero_sudo res.users views compile and render correctly.
        """
        # Execute get_view to satisfy the AST linter for xpath injections
        self.env['res.users'].get_view(view_type='form')
        self.env['res.users'].get_view(view_type='search')

    def test_02_zero_sudo_tour(self):
        # [@ANCHOR: test_zero_sudo_tour]
        # Tests [@ANCHOR: story_login_blocking]
        # Tests [@ANCHOR: journey_service_account_lifecycle]
        # Tests [@ANCHOR: zero_sudo_tour]
        """Run the zero_sudo_tour to verify UI functionality."""
        # Use the security utility to safely check for installed modules without violating Zero-Sudo
        utils = self.env['zero_sudo.security.utils']
        facility_uid = utils._get_service_uid("zero_sudo.odoo_facility_service_internal")
        installed_modules = self.env['ir.module.module'].with_user(facility_uid).search([('state', '=', 'installed')]).mapped('name')

        if 'hams_test' not in installed_modules:
            self.skipTest("hams_test module not installed, skipping tour that depends on its utilities.")
        self.start_tour("/odoo", "zero_sudo_tour", login="admin")
