# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""SiteResponse ends an owner-site request with a response of its own and answers_site_response turns it into the route's value."""
import werkzeug.exceptions
from odoo.tests import tagged
from odoo.tests.common import BaseCase

from odoo.addons.user_websites.controllers.owner_site import SiteResponse, answers_site_response


@tagged("post_install", "-at_install")
class TestSiteResponse(BaseCase):
    # Tests [@ANCHOR: user_websites_controllers_owner_site_site_response]
    # Tests [@ANCHOR: user_websites:fn_site_response_init]
    def test_it_carries_its_response_and_is_not_an_http_exception(self):
        response = object()
        ended = SiteResponse(response)
        self.assertIs(ended.response, response)
        self.assertEqual(str(ended), "owner site response")
        self.assertIsInstance(ended, Exception)
        self.assertNotIsInstance(ended, werkzeug.exceptions.HTTPException, "Odoo's error handler would replace its response")

    # Tests [@ANCHOR: user_websites_controllers_owner_site_site_response]
    # Tests [@ANCHOR: user_websites:fn_answers_site_response]
    def test_a_route_that_raises_it_returns_the_carried_response_and_other_errors_pass_through(self):
        response = object()

        @answers_site_response
        def ends_early(self_, value):
            raise SiteResponse(response)

        @answers_site_response
        def answers(self_, value):
            return value

        @answers_site_response
        def fails(self_, value):
            raise ValueError("not a site response")

        self.assertIs(ends_early(None, 1), response)
        self.assertEqual(answers(None, 7), 7)
        with self.assertRaises(ValueError):
            fails(None, 1)
