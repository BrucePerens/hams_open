# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
from odoo.tests import tagged

from odoo.addons.hams_base.models.ir_http import _has_nul
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestNulInRequestParams(HamsHttpCase):
    def test_01_the_check_finds_a_nul_anywhere_in_the_data(self):
        # Tests [@ANCHOR: hams_base:has_nul]
        self.assertTrue(_has_nul("a\x00b"))
        self.assertTrue(_has_nul({"q": ["x", {"deep": "a\x00"}]}))
        self.assertTrue(_has_nul({"k\x00": 1}))
        self.assertFalse(_has_nul({"q": ["x", ("y", 5, None, b"\x00")]}))
        nested = "ok"
        for _ in range(50000):
            nested = [nested]
        self.assertFalse(_has_nul(nested))

    def test_02_a_nul_in_a_query_parameter_is_a_400(self):
        # Tests [@ANCHOR: hams_base:refuse_nul_in_request_params]
        self.assertEqual(self.url_open("/web/login?redirect=%2Fmy").status_code, 200)
        self.assertEqual(self.url_open("/web/login?redirect=a%00b").status_code, 400)
