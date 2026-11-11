# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""A Cloudflare answer that is HTTP 200 but not the JSON the helper expects (an error page from a proxy, a body
cut off, a `result` of null) is a failed call, never an exception out of the helper and never a success."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged
from odoo.addons.cloudflare.utils import cloudflare_api as api

ZONE = "a" * 32
ACCOUNT = "b" * 32


class _Answer:
    status_code = 200

    def __init__(self, body=None, not_json=False):
        self._body, self._not_json = body, not_json

    def json(self):
        if self._not_json:
            raise ValueError("Expecting value")
        return self._body


@tagged("post_install", "-at_install")
class TestCloudflareApiUnreadableAnswers(HamsTransactionCase):
    def _each_bad_answer(self):
        """Three unreadable 200 answers: not JSON, JSON that is not an object, and a null result."""
        yield _Answer(not_json=True)
        yield _Answer(["not", "an", "object"])
        yield _Answer({"result": None})

    def test_01_object_result_calls_fail_instead_of_raising(self):
        mock = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")
        calls = {
            "ban_ip": lambda: api.ban_ip("192.0.2.1", "block", "n", "t", ZONE),
            "create_cfd_tunnel": lambda: api.create_cfd_tunnel(ACCOUNT, "t", "name"),
            "create_custom_hostname": lambda: api.create_custom_hostname("a.example.org", "t", ZONE),
            "get_custom_hostname": lambda: api.get_custom_hostname("h1", "t", ZONE),
            "get_cfd_tunnel_token": lambda: api.get_cfd_tunnel_token(ACCOUNT, "t", "tun"),
            "get_zone_details": lambda: api.get_zone_details(ZONE, "t"),
            "find_zone_id": lambda: api.find_zone_id("hams.com", "t"),
            "list_dns_records_named": lambda: api.list_dns_records_named(ZONE, "a.hams.com", "t"),
            "create_dns_record": lambda: api.create_dns_record(ZONE, {}, "t"),
            "count_dns_records": lambda: api.count_dns_records(ZONE, "t"),
        }
        for name, call in calls.items():
            for answer in self._each_bad_answer():
                mock.return_value = answer
                if name in ("find_zone_id", "list_dns_records_named", "create_dns_record", "get_zone_details") and answer._body == {"result": None}:
                    continue  # a null result is a legitimate empty answer for these
                with self.subTest(call=name, answer=answer._body, not_json=answer._not_json):
                    self.assertEqual(call()[0], False)

    def test_02_zone_and_tunnel_reads_return_their_empty_value(self):
        mock = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")
        for answer in self._each_bad_answer():
            mock.return_value = answer
            with self.subTest(answer=answer._body, not_json=answer._not_json):
                self.assertIsNone(api.get_zone_ruleset("http_request_firewall_custom", "t", ZONE))
                self.assertEqual(api.get_zone_ruleset_checked("http_request_firewall_custom", "t", ZONE)[0], "error")
                self.assertIsNone(api.get_zone_settings("t", ZONE))
                self.assertEqual(api.list_cfd_tunnels(ACCOUNT, "t"), [])

    def test_03_readable_answers_still_work(self):
        mock = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")
        mock.return_value = _Answer({"result": {"id": "r1"}})
        self.assertEqual(api.ban_ip("192.0.2.1", "block", "n", "t", ZONE), (True, "r1"))
        self.assertEqual(api.create_cfd_tunnel(ACCOUNT, "t", "name"), (True, "r1"))
        mock.return_value = _Answer({"result": "run-token"})
        self.assertEqual(api.get_cfd_tunnel_token(ACCOUNT, "t", "tun"), (True, "run-token"))
        mock.return_value = _Answer({"result": [{"id": "security_level", "value": "high"}]})
        self.assertEqual(api.get_zone_settings("t", ZONE)[0]["value"], "high")
        self.assertEqual(api.list_cfd_tunnels(ACCOUNT, "t")[0]["id"], "security_level")
