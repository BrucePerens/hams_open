# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The HTTP helpers of `utils/cloudflare_api.py` that the models mock at their own call sites, called for real
with `_make_request` replaced: the method, the endpoint, the payload and the return value of each one."""
import requests

from odoo.addons.cloudflare.utils import cloudflare_api as api
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger

ZONE = "a" * 32
ACCOUNT = "b" * 32
BASE = "https://api.cloudflare.com/client/v4"


class _Answer:
    def __init__(self, body=None, status=200):
        self._body, self.status_code = body, status

    def json(self):
        return self._body


@tagged("post_install", "-at_install")
class TestCloudflareApiCalls(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.mock = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")

    def _sent(self):
        """(method, endpoint, json payload or None) of the one request the helper made."""
        self.assertEqual(self.mock.call_count, 1)
        args, kwargs = self.mock.call_args
        return args[0], args[1], kwargs.get("json")

    def test_01_ban_and_unban_use_the_access_rules_endpoint(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_ban_ip]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_unban_ip]
        self.mock.return_value = _Answer({"result": {"id": "rule1"}})
        self.assertEqual(api.ban_ip("192.0.2.9", "block", "honeypot", "t", ZONE), (True, "rule1"))
        method, endpoint, payload = self._sent()
        self.assertEqual((method, endpoint), ("POST", f"{BASE}/zones/{ZONE}/firewall/access_rules/rules"))
        self.assertEqual(payload, {"mode": "block", "configuration": {"target": "ip", "value": "192.0.2.9"},
                                   "notes": "honeypot"})
        self.assertEqual(api.ban_ip("192.0.2.9", "block", "n", None, ZONE), (False, "Missing credentials"))
        self.mock.reset_mock()
        self.mock.return_value = _Answer({"result": {}})
        self.assertEqual(api.unban_ip("rule1", "t", ZONE), (True, "Success"))
        self.assertEqual(self._sent()[:2], ("DELETE", f"{BASE}/zones/{ZONE}/firewall/access_rules/rules/rule1"))
        self.mock.return_value = None
        self.assertEqual(api.unban_ip("rule1", "t", ZONE), (False, "API Error"))
        self.assertEqual(api.unban_ip("rule1", "t", ""), (False, "Missing credentials"))

    def test_02_verify_turnstile_posts_the_secret_and_the_visitor_address(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_verify_turnstile]
        post = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api.session.post")
        post.return_value = _Answer({"success": True})
        post.return_value.raise_for_status = lambda: None
        self.assertTrue(api.verify_turnstile("tok", "192.0.2.5", "sec"))
        self.assertEqual(post.call_args.kwargs["data"], {"secret": "sec", "response": "tok", "remoteip": "192.0.2.5"})
        self.assertFalse(api.verify_turnstile("", "192.0.2.5", "sec"))
        self.assertFalse(api.verify_turnstile("tok", None, ""))
        post.return_value = _Answer({"success": False})
        post.return_value.raise_for_status = lambda: None
        self.assertFalse(api.verify_turnstile("tok", None, "sec"))
        self.assertNotIn("remoteip", post.call_args.kwargs["data"])
        post.side_effect = requests.exceptions.ConnectionError("down")
        with mute_logger("odoo.addons.cloudflare.utils.cloudflare_api"):
            self.assertFalse(api.verify_turnstile("tok", None, "sec"))

    def test_03_ruleset_reads_tell_absent_from_unreadable(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_get_zone_ruleset]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_get_zone_ruleset_checked]
        phase = "http_request_cache_settings"
        self.mock.return_value = _Answer({"result": {"id": "rs1", "rules": []}})
        self.assertEqual(api.get_zone_ruleset(phase, "t", ZONE)["id"], "rs1")
        self.assertEqual(self._sent()[:2], ("GET", f"{BASE}/zones/{ZONE}/rulesets/phases/{phase}/entrypoint"))
        self.assertEqual(api.get_zone_ruleset_checked(phase, "t", ZONE)[0], "ok")
        self.mock.return_value = _Answer({}, status=404)
        self.assertIsNone(api.get_zone_ruleset(phase, "t", ZONE))
        self.assertEqual(api.get_zone_ruleset_checked(phase, "t", ZONE), ("absent", None))
        self.mock.return_value = _Answer({}, status=403)
        self.assertEqual(api.get_zone_ruleset_checked(phase, "t", ZONE), ("error", "HTTP 403"))
        self.mock.return_value = None
        self.assertIsNone(api.get_zone_ruleset(phase, "t", ZONE))
        self.assertEqual(api.get_zone_ruleset_checked(phase, "t", ZONE), ("error", "API Error"))
        self.assertEqual(api.get_zone_ruleset_checked(phase, "", ZONE)[0], "error")
        self.assertIsNone(api.get_zone_ruleset(phase, "t", ""))

    def test_04_ruleset_writes_put_to_the_ruleset_and_post_to_the_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_update_zone_ruleset]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_create_zone_ruleset]
        self.mock.return_value = _Answer({"result": {}})
        self.assertEqual(api.update_zone_ruleset("rs1", {"rules": []}, "t", ZONE), (True, "Ruleset updated successfully."))
        self.assertEqual(self._sent(), ("PUT", f"{BASE}/zones/{ZONE}/rulesets/rs1", {"rules": []}))
        self.mock.reset_mock()
        self.assertEqual(api.create_zone_ruleset({"name": "x"}, "t", ZONE), (True, "Ruleset created successfully."))
        self.assertEqual(self._sent(), ("POST", f"{BASE}/zones/{ZONE}/rulesets", {"name": "x"}))
        self.mock.return_value = None
        self.assertEqual(api.update_zone_ruleset("rs1", {}, "t", ZONE), (False, "API Error"))
        self.assertEqual(api.create_zone_ruleset({}, "t", ZONE), (False, "API Error"))
        self.assertEqual(api.update_zone_ruleset("rs1", {}, "", ZONE), (False, "Missing credentials."))
        self.assertEqual(api.create_zone_ruleset({}, "t", ""), (False, "Missing credentials."))

    def test_05_tunnel_create_token_list_and_delete(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_create_cfd_tunnel]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_get_cfd_tunnel_token]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_list_cfd_tunnels]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_delete_cfd_tunnel]
        self.mock.return_value = _Answer({"result": {"id": "tun1"}})
        self.assertEqual(api.create_cfd_tunnel(ACCOUNT, "t", "edge"), (True, "tun1"))
        method, endpoint, payload = self._sent()
        self.assertEqual((method, endpoint), ("POST", f"{BASE}/accounts/{ACCOUNT}/cfd_tunnel"))
        self.assertEqual(payload["name"], "edge")
        self.assertGreaterEqual(len(payload["tunnel_secret"]), 43)  # 32 random bytes, base64
        self.mock.reset_mock()
        self.mock.return_value = _Answer({"result": "run-token"})
        self.assertEqual(api.get_cfd_tunnel_token(ACCOUNT, "t", "tun1"), (True, "run-token"))
        self.assertEqual(self._sent()[:2], ("GET", f"{BASE}/accounts/{ACCOUNT}/cfd_tunnel/tun1/token"))
        self.mock.reset_mock()
        self.mock.return_value = _Answer({"result": [{"id": "tun1"}]})
        self.assertEqual(api.list_cfd_tunnels(ACCOUNT, "t"), [{"id": "tun1"}])
        self.assertEqual(self._sent()[:2], ("GET", f"{BASE}/accounts/{ACCOUNT}/cfd_tunnel"))
        self.mock.reset_mock()
        self.mock.return_value = _Answer({"result": {}})
        self.assertEqual(api.delete_cfd_tunnel(ACCOUNT, "t", "tun1"), (True, "Tunnel deleted successfully."))
        self.assertEqual(self._sent()[:2], ("DELETE", f"{BASE}/accounts/{ACCOUNT}/cfd_tunnel/tun1"))
        self.mock.return_value = None
        self.assertEqual(api.create_cfd_tunnel(ACCOUNT, "t", "edge"), (False, "API Error"))
        self.assertEqual(api.get_cfd_tunnel_token(ACCOUNT, "t", "tun1"), (False, "API Error"))
        self.assertEqual(api.list_cfd_tunnels(ACCOUNT, "t"), [])
        self.assertEqual(api.delete_cfd_tunnel(ACCOUNT, "t", "tun1"), (False, "API Error"))
        self.assertEqual(api.delete_cfd_tunnel(ACCOUNT, "t", ""), (False, "Missing credentials or tunnel ID"))
        self.assertEqual(api.create_cfd_tunnel("", "t", "edge"), (False, "Missing credentials"))
        self.assertEqual(api.get_cfd_tunnel_token("", "t", "tun1"), (False, "Missing credentials"))
        self.assertEqual(api.list_cfd_tunnels("", "t"), [])

    def test_06_purge_everything_posts_the_purge_flag(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_purge_everything]
        self.mock.return_value = _Answer({"success": True})
        self.assertTrue(api.purge_everything("t", ZONE))
        self.assertEqual(self._sent(), ("POST", f"{BASE}/zones/{ZONE}/purge_cache", {"purge_everything": True}))
        self.mock.return_value = None
        self.assertFalse(api.purge_everything("t", ZONE))
        self.assertFalse(api.purge_everything("", ZONE))
        self.assertFalse(api.purge_everything("t", ""))

    def test_07_purge_urls_sends_chunks_of_thirty(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_purge_urls]
        self.mock.return_value = _Answer({"success": True})
        self.assertTrue(api.purge_urls([f"https://a.example/{i}" for i in range(65)], "t", ZONE))
        self.assertEqual([len(c.kwargs["json"]["files"]) for c in self.mock.call_args_list], [30, 30, 5])
        self.mock.return_value = None
        self.assertFalse(api.purge_urls(["https://a.example/"], "t", ZONE))
        self.assertTrue(api.purge_urls([], "t", ZONE))
        self.assertFalse(api.purge_urls(["https://a.example/"], "", ZONE))

    def test_08_zone_settings_read_and_patch(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_get_zone_settings]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_update_zone_setting]
        self.mock.return_value = _Answer({"result": [{"id": "security_level", "value": "high"}]})
        self.assertEqual(api.get_zone_settings("t", ZONE), [{"id": "security_level", "value": "high"}])
        self.assertEqual(self._sent()[:2], ("GET", f"{BASE}/zones/{ZONE}/settings"))
        self.mock.return_value = _Answer({}, status=404)
        self.assertIsNone(api.get_zone_settings("t", ZONE))
        self.assertIsNone(api.get_zone_settings("t", ""))
        self.mock.reset_mock()
        self.mock.return_value = _Answer({"result": {}})
        self.assertEqual(api.update_zone_setting("always_use_https", "on", "t", ZONE), (True, "Setting updated successfully."))
        self.assertEqual(self._sent(), ("PATCH", f"{BASE}/zones/{ZONE}/settings/always_use_https", {"value": "on"}))
        self.mock.return_value = None
        self.assertEqual(api.update_zone_setting("always_use_https", "on", "t", ZONE), (False, "API Error"))
        self.assertEqual(api.update_zone_setting("x", "on", "", ZONE), (False, "Missing credentials"))

    def test_09_custom_hostname_create_get_delete(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_create_custom_hostname]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_get_custom_hostname]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_delete_custom_hostname]
        self.mock.return_value = _Answer({"result": {"id": "h1", "ssl": {"status": "pending_validation"}}})
        ok, result = api.create_custom_hostname("club.example.org", "t", ZONE)
        self.assertEqual((ok, result["id"]), (True, "h1"))
        method, endpoint, payload = self._sent()
        self.assertEqual((method, endpoint), ("POST", f"{BASE}/zones/{ZONE}/custom_hostnames"))
        self.assertEqual((payload["hostname"], payload["ssl"]["method"]), ("club.example.org", "http"))
        self.mock.reset_mock()
        self.assertEqual(api.get_custom_hostname("h1", "t", ZONE)[1]["id"], "h1")
        self.assertEqual(self._sent()[:2], ("GET", f"{BASE}/zones/{ZONE}/custom_hostnames/h1"))
        self.mock.reset_mock()
        self.assertEqual(api.delete_custom_hostname("h1", "t", ZONE), (True, "Custom hostname deleted successfully."))
        self.assertEqual(self._sent()[:2], ("DELETE", f"{BASE}/zones/{ZONE}/custom_hostnames/h1"))
        self.mock.return_value = _Answer({}, status=404)
        self.assertEqual(api.delete_custom_hostname("h1", "t", ZONE), (True, "Custom hostname was already gone."))
        self.mock.return_value = None
        self.assertEqual(api.delete_custom_hostname("h1", "t", ZONE), (False, "API Error"))
        self.assertEqual(api.create_custom_hostname("a.example.org", "t", ZONE), (False, "API Error"))
        self.assertEqual(api.get_custom_hostname("h1", "t", ZONE), (False, "API Error"))
        self.assertEqual(api.create_custom_hostname("", "t", ZONE), (False, "Missing credentials or hostname"))
        self.assertEqual(api.get_custom_hostname("", "t", ZONE), (False, "Missing credentials or hostname ID"))
        self.assertEqual(api.delete_custom_hostname("h1", "t", ""), (False, "Missing credentials or hostname ID"))
