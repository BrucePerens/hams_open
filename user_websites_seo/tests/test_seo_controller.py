# -*- coding: utf-8 -*-
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.http import Response
from odoo.addons.user_websites_seo.controllers.main import UserWebsitesSEOController

class TestSEOController(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.regular_user = self.env['res.users'].create({
            'name': 'SEO Test User',
            'login': 'seo_test',
            'website_slug': 'seo-test-user',
            'group_ids': [(6, 0, [self.env.ref('base.group_portal').id])]
        })
        self.controller = UserWebsitesSEOController()

    def test_controller_no_ssti_elevation(self):
        # Tests [@ANCHOR: controller_user_blog_index_seo_override]
        # [@ANCHOR: test_controller_no_ssti_elevation]
        # Verified by [@ANCHOR: test_controller_no_ssti_elevation]
        """
        Verify the controller intercepts the QWeb context and injects
        the main_object without elevating privileges (no sudo/svc_uid),
        to avoid SSTI vulnerabilities.
        """
        mock_super_index = self.safe_patch('odoo.addons.user_websites_seo.controllers.main.UserWebsitesController.user_blog_index')

        # Mock the response from the base controller
        mock_response = Response()
        mock_response.type = 'http'
        mock_response.qcontext = {
            'profile_user': self.regular_user
        }
        mock_super_index.return_value = mock_response

        # Call the controller method
        response = self.controller.user_blog_index('seo-test-user')

        # Check that main_object is set
        self.assertIn('main_object', response.qcontext)

        # Check that it is exactly the same recordset (no with_user or sudo)
        main_obj = response.qcontext['main_object']
        self.assertEqual(main_obj.env.uid, self.regular_user.env.uid, "The main_object should not have elevated privileges (SSTI protection)")
        self.assertEqual(main_obj, self.regular_user)
