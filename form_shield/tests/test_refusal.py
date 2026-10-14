# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged

from .test_support import reset_shield_state, shield_fields
from ..models import form_shield as shield_module
from ..refusal import PAGE_STATUS, _with_query

BAD_ANSWER = "v1.pow.1.2.3.aa.bb.cc~1"


@tagged("post_install", "-at_install", "form_shield")
class TestRefusalResponse(HamsHttpCase):
    """refusal_response() as the hams_com report forms use it, through a route that is not one of
    them: Odoo's own /website/form is covered in test_http, the report pages in their modules."""

    def setUp(self):
        super().setUp()
        reset_shield_state(self.env)

    # [@ANCHOR: form_shield:test_refusal_response_by_outcome]
    def test_refusal_response_by_outcome(self):
        # Tests [@ANCHOR: form_shield:with_query]
        # Tests [@ANCHOR: form_shield:refusal_response]
        # Tests [@ANCHOR: form_shield:refused_page]
        self.assertEqual(_with_query("/p", "a=1"), "/p?a=1")
        self.assertEqual(_with_query("/p?x=2", "a=1"), "/p?x=2&a=1")
        self.assertEqual(
            PAGE_STATUS,
            {shield_module.LIMITED: 429, shield_module.UNAVAILABLE: 503},
        )
        self.assertTrue(shield_fields(self.env, "contact"))
