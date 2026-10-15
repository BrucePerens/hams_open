# SPDX-License-Identifier: AGPL-3.0-or-later
import re
import time

import odoo

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged

from .. import tokens
from ..models.ir_http import ENFORCED_ACTIONS
from .test_support import LOOPBACK, reset_shield_state, shield_response


@tagged("post_install", "-at_install", "form_shield")
class TestFormShieldHttp(HamsHttpCase):
    def setUp(self):
        super().setUp()
        reset_shield_state(self.env)

    # [@ANCHOR: form_shield:test_challenge_endpoint_issues_signed_uncacheable_json]
    def test_challenge_endpoint_issues_signed_uncacheable_json(self):
        # Tests [@ANCHOR: form_shield:client_address]
        # Tests [@ANCHOR: form_shield:challenge_route]
        # Tests [@ANCHOR: form_shield:issue]
        response = self.url_open("/form_shield/challenge", data={"kind": "pow"}, method="POST")
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.headers["Cache-Control"], "no-store")
        self.assertNotIn("Set-Cookie", response.headers)
        issued = response.json()
        key = self.env["form.shield"]._signing_key()
        parsed, reason = tokens.parse_challenge(key, issued["challenge"])
        self.assertIsNone(reason)
        self.assertEqual(issued["kind"], "pow")
        self.assertGreater(issued["difficulty"], 0)
        # Two requests never get the same challenge.
        again = self.url_open("/form_shield/challenge", data={}, method="POST").json()
        self.assertNotEqual(again["challenge"], issued["challenge"])
        question = self.url_open("/form_shield/challenge", data={"kind": "q"}, method="POST").json()
        self.assertEqual(question["kind"], "q")
        self.assertEqual(question["difficulty"], 0)
        self.assertTrue(question["question"].startswith("What is"))
        # A GET is not an issue route: pages that may be edge-cached must never share one.
        self.assertEqual(self.url_open("/form_shield/challenge").status_code, 405)

    def test_challenge_is_bound_to_the_forwarded_network(self):
        # Tests [@ANCHOR: form_shield:ip_tag]
        # Behind the tunnel the peer is loopback and the visitor is in CF-Connecting-IP.
        key = self.env["form.shield"]._signing_key()
        headers = {"CF-Connecting-IP": "198.51.100.77"}
        issued = self.url_open(
            "/form_shield/challenge", data={}, headers=headers, method="POST"
        ).json()
        parsed, _ = tokens.parse_challenge(key, issued["challenge"])
        self.assertEqual(parsed.tag, tokens.ip_tag(key, "198.51.100.200"))
        self.assertNotEqual(parsed.tag, tokens.ip_tag(key, LOOPBACK))

    # [@ANCHOR: form_shield:test_odoo_forms_require_a_solved_challenge]
    def test_odoo_forms_require_a_solved_challenge(self):
        # Tests [@ANCHOR: form_shield:captcha_hook]
        # Tests [@ANCHOR: form_shield:recaptcha_patch]
        self.authenticate(None, None)
        data = {"email_from": "visitor@example.com", "subject": "Hello", "body_html": "A question about the site", "website_form_signature": ""}
        refused = self.url_open("/website/form/mail.mail", data=data, method="POST")
        self.assertIn("reload the page", refused.text)
        forged = dict(data, recaptcha_token_response="v1.pow.1.2.3.aa.bb.cc~1")
        refused = self.url_open("/website/form/mail.mail", data=forged, method="POST")
        self.assertIn("could not verify", refused.text)
        solved = dict(
            data, recaptcha_token_response=shield_response(self.env, "website_form")
        )
        allowed = self.url_open("/website/form/mail.mail", data=solved, method="POST")
        # Past the shield the form's own rules answer; none of them is ours.
        self.assertNotIn("verify this submission", allowed.text)
        self.assertNotIn("reload the page", allowed.text)
        # A solution for another form does not open this one.
        other = dict(data, recaptcha_token_response=shield_response(self.env, "report_violation"))
        refused = self.url_open("/website/form/mail.mail", data=other, method="POST")
        self.assertIn("could not verify", refused.text)

    def test_full_browser_flow_in_python(self):
        """The whole round trip a browser makes: fetch, solve, wait out the minimum age, post."""
        # Tests [@ANCHOR: form_shield:check]
        # Tests [@ANCHOR: form_shield:pow_valid]
        self.authenticate(None, None)
        issued = self.url_open("/form_shield/challenge", data={}, method="POST").json()
        started = time.time()
        counter = tokens.solve_pow(issued["challenge"], "website_form", issued["difficulty"])
        data = {"email_from": "v@example.com", "subject": "S", "body_html": "Body of the message",
                "website_form_signature": "",
                "recaptcha_token_response": f"{issued['challenge']}~{counter}"}
        # Wait out whatever part of the minimum age the solving did not already use.
        remaining = issued["min_age"] + 1 - (time.time() - started)
        if remaining > 0:
            time.sleep(remaining)
        on_time = self.url_open("/website/form/mail.mail", data=data, method="POST")
        self.assertNotIn("verify this submission", on_time.text)
        self.assertNotIn("timed out", on_time.text)
        again = self.url_open("/website/form/mail.mail", data=data, method="POST")
        self.assertIn("could not verify", again.text)

    def test_enforced_actions_match_the_browser_patch(self):
        """The server enforces exactly the actions the browser patch answers. An action enforced
        here that the patch does not answer would refuse every visitor; one answered there but
        not enforced here would only slow its form down."""
        # Tests [@ANCHOR: form_shield:captcha_hook]
        # Tests [@ANCHOR: form_shield:recaptcha_patch]
        path = odoo.tools.file_path("form_shield/static/src/js/recaptcha_patch.js")
        with open(path, encoding="utf-8") as handle:
            source = handle.read()
        block = re.search(r"new Set\(\[(.*?)\]\)", source, re.S).group(1)
        client_actions = tuple(re.findall(r'"([a-z_]+)"', block))
        self.assertEqual(sorted(client_actions), sorted(ENFORCED_ACTIONS))
