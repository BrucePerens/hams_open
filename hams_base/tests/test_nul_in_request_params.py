# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
import json
import re

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

    def test_03_a_nul_in_a_form_field_is_a_400(self):
        # Tests [@ANCHOR: hams_base:refuse_nul_in_request_params]
        # The control carries a valid CSRF token, so a 400 can only come from the NUL.
        page = self.url_open("/web/login").text
        token = re.search(r'name="csrf_token" value="([^"]+)"', page).group(1)
        data = {"login": "nobody", "password": "x", "csrf_token": token}
        self.assertNotEqual(self.url_open("/web/login", data=data).status_code, 400)
        data["login"] = "no\x00body"
        self.assertEqual(self.url_open("/web/login", data=data).status_code, 400)

    def test_04_a_nul_in_a_json_body_is_a_400(self):
        # Tests [@ANCHOR: hams_base:refuse_nul_in_request_params]
        # A JSON-RPC route answers an error with status 200, so the 400 is read from the error body.
        def call(params):
            response = self.url_open(
                "/web/database/list",
                data=json.dumps({"jsonrpc": "2.0", "method": "call", "params": params}),
                headers={"Content-Type": "application/json"},
            )
            return response.json().get("error", {}).get("code")

        self.assertNotEqual(call({"q": ["x"]}), 400)
        self.assertEqual(call({"q": ["x", {"deep": "a\x00b"}]}), 400)
