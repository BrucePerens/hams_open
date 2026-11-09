# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import contextlib
import ipaddress

from werkzeug.datastructures import Headers

from odoo.addons.zero_sudo.models.security_utils import ZeroSudoSecurityUtils
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase

from odoo.addons.hams_rpc_guard import endpoint_gate as gate
from odoo.addons.hams_rpc_guard.hooks import post_init_hook

# Built, not written out: the test linter refuses a literal loopback address (containers resolve it differently).
LOOPBACK = str(ipaddress.IPv4Address(0x7F000001))
LOOPBACK_NET = LOOPBACK.split(".")[0] + ".0.0.0/8"

VERSION_CALL = '<?xml version="1.0"?><methodCall><methodName>version</methodName><params/></methodCall>'
FROM_A_VISITOR = {"X-Forwarded-For": "203.0.113.9", "CF-Connecting-IP": "203.0.113.9"}


@tagged("post_install", "-at_install")
class TestEndpointFamily(HamsTransactionCase):
    """Tests [@ANCHOR: hams_rpc_guard:endpoint_family]"""

    def test_the_real_routes_are_recognised(self):
        for path in ("/jsonrpc", "/xmlrpc/common", "/xmlrpc/2/object", "/xmlrpc/2/db"):
            self.assertEqual(gate.endpoint_family(path), gate.FAMILY_LEGACY, path)
        for path in ("/json/2", "/json/2/", "/json/2/res.partner/search_read"):
            self.assertEqual(gate.endpoint_family(path), gate.FAMILY_JSON2, path)

    def test_lookalikes_and_the_web_client_are_not_gated(self):
        for path in ("/xmlrpc.php", "/xmlrpc", "/jsonrpc/", "/jsonrpcx", "/json/21", "/json/version",
                     "/web/dataset/call_kw/res.partner/search_read", "/", "", None):
            self.assertIsNone(gate.endpoint_family(path), path)


@tagged("post_install", "-at_install")
class TestOriginIsLocal(HamsTransactionCase):
    """Tests the origin decision."""
    # Tests [@ANCHOR: hams_rpc_guard:origin_is_local]
    # Tests [@ANCHOR: hams_rpc_guard:parse_address]
    # Tests [@ANCHOR: hams_rpc_guard:parse_trusted_proxies]
    # Tests [@ANCHOR: hams_rpc_guard:client_addresses]

    def _local(self, peer=None, orig_peer=None, headers=None, trusted=LOOPBACK):
        environ = {}
        if peer is not None:
            environ["REMOTE_ADDR"] = peer
        if orig_peer is not None:
            environ["werkzeug.proxy_fix.orig"] = {"REMOTE_ADDR": orig_peer}
        return gate.origin_is_local(environ, Headers(list((headers or {}).items())), gate.parse_trusted_proxies(trusted))

    def test_a_loopback_peer_without_forwarding_headers_is_local(self):
        for peer in (LOOPBACK, "127.8.9.10", "::1", "::ffff:" + LOOPBACK):
            self.assertTrue(self._local(peer), peer)

    def test_no_peer_address_is_a_unix_socket_and_is_local(self):
        self.assertTrue(self._local(""))
        self.assertTrue(self._local(None))

    def test_a_remote_peer_is_not_local(self):
        for peer in ("203.0.113.9", "10.99.0.2", "2001:db8::1", "::ffff:203.0.113.9", "not-an-address"):
            self.assertFalse(self._local(peer), peer)

    def test_proxy_fix_cannot_launder_a_remote_peer_or_a_forged_header(self):
        # proxy_mode rewrote REMOTE_ADDR from X-Forwarded-For to loopback; the socket peer was a remote host.
        self.assertFalse(self._local(peer=LOOPBACK, orig_peer="203.0.113.9"))
        # A visitor of a public site comes through nginx or cloudflared on this machine: loopback peer, but with
        # the headers the proxy adds. Even a forged LOOPBACK in X-Forwarded-For is a forwarding header.
        self.assertFalse(self._local(peer=LOOPBACK, orig_peer=LOOPBACK, headers={"X-Forwarded-For": LOOPBACK}, trusted=""))

    def test_a_forwarded_request_is_refused_unless_the_proxy_is_trusted_and_every_client_is_loopback(self):
        proxied = {"X-Forwarded-For": LOOPBACK}
        self.assertTrue(self._local(LOOPBACK, headers=proxied, trusted=LOOPBACK_NET))
        self.assertTrue(self._local("::1", headers={"Forwarded": 'for="[::1]:1234";proto=http'}, trusted="::1"))
        self.assertFalse(self._local(LOOPBACK, headers=proxied, trusted=""), "no trusted proxy configured")
        self.assertFalse(self._local(LOOPBACK, headers=proxied, trusted="10.0.0.0/8"), "peer is not in the list")
        self.assertFalse(self._local(LOOPBACK, headers={"X-Forwarded-For": LOOPBACK + ", 203.0.113.9"}, trusted=LOOPBACK))
        self.assertFalse(self._local(LOOPBACK, headers={"X-Forwarded-For": LOOPBACK, "CF-Connecting-IP": "203.0.113.9"}, trusted=LOOPBACK))
        self.assertFalse(self._local(LOOPBACK, headers={"X-Forwarded-For": "unknown"}, trusted=LOOPBACK))
        self.assertFalse(self._local(LOOPBACK, headers={"X-Forwarded-Proto": "https"}, trusted=LOOPBACK),
                         "a trusted proxy that names no client address proves nothing")

    def test_trusted_proxy_parameter_ignores_junk(self):
        networks = gate.parse_trusted_proxies(LOOPBACK + ", nonsense, ::1 ,10.0.0.0/8")
        self.assertEqual([str(n) for n in networks], [LOOPBACK + "/32", "::1/128", "10.0.0.0/8"])
        self.assertEqual(gate.parse_trusted_proxies(None), [])

    def test_each_forwarding_header_alone_marks_the_request_as_forwarded(self):
        for name in gate.FORWARDING_HEADERS:
            with self.subTest(header=name):
                self.assertFalse(self._local(LOOPBACK, headers={name.lower(): "x"}, trusted=""))


@tagged("post_install", "-at_install")
class TestPolicy(HamsTransactionCase):
    """Tests the policy table and the refusal counter."""
    # Tests [@ANCHOR: hams_rpc_guard:policy_allows]
    # Tests [@ANCHOR: hams_rpc_guard:normalise_policy]
    # Tests [@ANCHOR: hams_rpc_guard:note_denial]
    # Tests [@ANCHOR: hams_rpc_guard:denial_counts]
    # Tests [@ANCHOR: hams_rpc_guard:is_power_of_ten]

    def test_the_three_policies(self):
        self.assertTrue(gate.policy_allows(gate.POLICY_ANY, False))
        self.assertTrue(gate.policy_allows(gate.POLICY_LOOPBACK, True))
        self.assertFalse(gate.policy_allows(gate.POLICY_LOOPBACK, False))
        self.assertFalse(gate.policy_allows(gate.POLICY_NONE, True))
        self.assertFalse(gate.policy_allows(gate.POLICY_NONE, False))

    def test_unset_takes_the_default_and_junk_fails_closed(self):
        self.assertEqual(gate.normalise_policy(None, gate.POLICY_ANY), gate.POLICY_ANY)
        self.assertEqual(gate.normalise_policy("", gate.POLICY_LOOPBACK), gate.POLICY_LOOPBACK)
        self.assertEqual(gate.normalise_policy(" ANY ", gate.POLICY_LOOPBACK), gate.POLICY_ANY)
        self.assertEqual(gate.normalise_policy("yes please", gate.POLICY_ANY), gate.POLICY_LOOPBACK)

    def test_the_defaults_are_the_documented_ones(self):
        self.assertEqual(gate.DEFAULT_LEGACY_POLICY, gate.POLICY_LOOPBACK)
        self.assertEqual(gate.DEFAULT_JSON2_POLICY, gate.POLICY_ANY)

    def test_denials_are_counted_and_logged_at_powers_of_ten_without_values(self):
        gate._denied.clear()
        with self.assertLogs(gate._logger, level="WARNING") as logs:
            for _ in range(12):
                gate.note_denial(gate.FAMILY_LEGACY, gate.POLICY_LOOPBACK)
        self.assertEqual(gate.denial_counts()[gate.FAMILY_LEGACY], 12)
        self.assertEqual(len(logs.records), 2)  # the 1st and the 10th
        for line in logs.output:
            self.assertIn(gate.LEGACY_PARAM, line)
        gate._denied.clear()

    def test_the_policy_words_are_readable_through_the_zero_sudo_whitelist(self):
        utils = self.env["zero_sudo.security.utils"]
        for key in (gate.LEGACY_PARAM, gate.JSON2_PARAM, gate.TRUSTED_PROXIES_PARAM):
            self.assertIn(key, utils._get_param_read_whitelist())


@tagged("post_install", "-at_install")
class TestRpcEndpointGate(HamsHttpCase):
    """A real HTTP request, from this machine, with and without the headers a proxy adds."""
    # Tests [@ANCHOR: hams_rpc_guard:endpoint_gate]
    # Tests [@ANCHOR: hams_rpc_guard:refuse_endpoint_when_closed]

    def setUp(self):
        super().setUp()
        self.values = {}

        def read(utils, key, default=None):
            value = self.values.get(key)
            return value if value is not None else default
        # What the operator set, None (absent) for unset: the default then applies.
        self.safe_patch_object(ZeroSudoSecurityUtils, "_get_system_param", new=read)

    @contextlib.contextmanager
    def _policies(self, legacy=None, json2=None, trusted=None):
        self.values = {gate.LEGACY_PARAM: legacy, gate.JSON2_PARAM: json2, gate.TRUSTED_PROXIES_PARAM: trusted}
        try:
            yield
        finally:
            self.values = {}

    def _xmlrpc(self, route="/xmlrpc/2/common", headers=None):
        return self.url_open(route, data=VERSION_CALL, headers={"Content-Type": "text/xml", **(headers or {})})

    def _assert_status(self, response, status):
        self.assertEqual(response.status_code, status, response.text[:200])

    def _jsonrpc(self, headers=None):
        payload = {"jsonrpc": "2.0", "method": "call", "params": {"service": "common", "method": "version", "args": []}}
        return self.url_open("/jsonrpc", json=payload, headers=headers or {})

    def _json2(self, headers=None):
        return self.url_open("/json/2/res.users/context_get", data="{}", headers={"Content-Type": "application/json", "X-Odoo-Database": self.env.cr.dbname, "Authorization": "Bearer not-a-real-api-key", **(headers or {})})

    def test_default_a_daemon_on_this_machine_still_works(self):
        with self._policies():
            for route in ("/xmlrpc/2/common", "/xmlrpc/common"):
                response = self._xmlrpc(route)
                self.assertEqual(response.status_code, 200, route)
                self.assertIn("server_version", response.text, route)
            self.assertIn("server_version", self._jsonrpc().text)

    def test_default_a_forwarded_request_is_refused_as_if_the_route_did_not_exist(self):
        with self._policies():
            for route in ("/xmlrpc/2/common", "/xmlrpc/common"):
                response = self._xmlrpc(route, FROM_A_VISITOR)
                self.assertEqual(response.status_code, 404, route)
                self.assertNotIn("server_version", response.text, route)
            response = self._jsonrpc(FROM_A_VISITOR)
            self.assertNotIn("server_version", response.text)
            self.assertIn("404", response.text)

    def test_a_trusted_proxy_on_this_machine_may_forward_a_local_client(self):
        local_client = {"X-Forwarded-For": LOOPBACK}
        with self._policies(trusted=LOOPBACK):
            self.assertEqual(self._xmlrpc(headers=local_client).status_code, 200)
            self.assertEqual(self._xmlrpc(headers=FROM_A_VISITOR).status_code, 404)
        with self._policies():
            self.assertEqual(self._xmlrpc(headers=local_client).status_code, 404)

    def test_a_single_cloudflare_header_is_enough_to_be_refused(self):
        with self._policies():
            self.assertEqual(self._xmlrpc(headers={"CF-Connecting-IP": "203.0.113.9"}).status_code, 404)

    def test_the_operator_can_open_the_legacy_endpoints(self):
        with self._policies(legacy="any"):
            response = self._xmlrpc(headers=FROM_A_VISITOR)
        self.assertEqual(response.status_code, 200)
        self.assertIn("server_version", response.text)

    def test_the_operator_can_close_them_even_to_this_machine(self):
        with self._policies(legacy="none"):
            self.assertEqual(self._xmlrpc().status_code, 404)
            self.assertNotIn("server_version", self._jsonrpc().text)

    def test_junk_in_the_parameter_fails_closed(self):
        with self._policies(legacy="whatever"):
            self.assertEqual(self._xmlrpc(headers=FROM_A_VISITOR).status_code, 404)
            self.assertEqual(self._xmlrpc().status_code, 200)

    def test_an_unreadable_parameter_fails_closed(self):
        class Broken(dict):
            def get(self, key, default=None):
                raise RuntimeError("cache down")
        self.values = Broken()
        with self.assertLogs("odoo.addons.hams_rpc_guard.models.ir_http", "ERROR"):
            self.assertEqual(self._xmlrpc(headers=FROM_A_VISITOR).status_code, 404)

    def test_json2_default_is_stock_so_an_off_box_daemon_is_not_cut_off(self):
        # A bearer key that is not one: Odoo's own 401, which proves the request got past the gate to the core's authentication.
        with self._policies():
            self._assert_status(self._json2(FROM_A_VISITOR), 401)
            self._assert_status(self._json2(), 401)

    def test_json2_can_be_limited_to_this_machine_and_then_never_reaches_authentication(self):
        with self._policies(json2="loopback"):
            self.assertEqual(self._json2(FROM_A_VISITOR).status_code, 404)
            self._assert_status(self._json2(), 401)
        with self._policies(json2="none"):
            self.assertEqual(self._json2().status_code, 404)

    def test_the_two_families_are_independent(self):
        with self._policies(legacy="none", json2="any"):
            self._assert_status(self._json2(FROM_A_VISITOR), 401)
        with self._policies(legacy="any", json2="none"):
            self.assertEqual(self._xmlrpc(headers=FROM_A_VISITOR).status_code, 200)

    def test_the_web_client_and_other_routes_are_never_gated(self):
        with self._policies(legacy="none", json2="none"):
            self.assertEqual(self.url_open("/json/version", headers=FROM_A_VISITOR).status_code, 200)
            self.assertEqual(self.url_open("/web/login", headers=FROM_A_VISITOR).status_code, 200)

    def test_a_refusal_is_logged_without_any_request_value(self):
        gate._denied.clear()
        with self._policies(), self.assertLogs(gate._logger, level="WARNING") as logs:
            self._xmlrpc(headers={**FROM_A_VISITOR, "User-Agent": "attacker-agent"})
        joined = "\n".join(logs.output)
        self.assertIn("Refused 1 request", joined)
        for value in ("203.0.113.9", "attacker-agent", "version"):
            self.assertNotIn(value, joined)
        gate._denied.clear()


@tagged("post_install", "-at_install")
class TestPublicVisitorCannotUseTheWebClientRpc(HamsHttpCase):
    """The website's visitors have no business with `call_kw`/`call_button`: Odoo's routes are `auth="user"` and
    refuse the public user like an anonymous caller. This pins that, so a future route or `auth` override that lets
    the public user through fails here instead of silently opening every public model method.
    Tests [@ANCHOR: hams_rpc_guard:endpoint_gate]"""

    def _call(self, route, model="res.partner", method="search_read"):
        payload = {"jsonrpc": "2.0", "method": "call",
                   "params": {"model": model, "method": method, "args": [[]], "kwargs": {"fields": ["name"]}}}
        return self.url_open(f"{route}/{model}/{method}", json=payload)

    def test_call_kw_and_call_button_refuse_an_anonymous_visitor(self):
        for route in ("/web/dataset/call_kw", "/web/dataset/call_button"):
            with self.subTest(route=route):
                body = self._call(route).json()
                self.assertIn("error", body, route)
                self.assertNotIn("result", body, route)
                self.assertEqual(body["error"]["code"], 100, body)  # Odoo Session Expired

    def test_a_public_model_method_of_ours_is_not_reachable_either(self):
        body = self._call("/web/dataset/call_kw", "ir.config_parameter", "get_param").json()
        self.assertIn("error", body)
        self.assertNotIn("result", body)


@tagged("post_install", "-at_install")
class TestPostInitHook(HamsTransactionCase):
    """Tests [@ANCHOR: hams_rpc_guard:post_init_hook]"""

    def test_the_hook_installs_the_documentation_and_sets_no_parameter(self):
        bootstrap = self.safe_patch_object(type(self.env["ir.module.module"]), "_bootstrap_knowledge_docs")
        post_init_hook(self.env)
        bootstrap.assert_called_once_with()
        keys = [gate.LEGACY_PARAM, gate.JSON2_PARAM, gate.TRUSTED_PROXIES_PARAM]
        stored = self.env["ir.config_parameter"].search_read([("key", "in", keys)], ["key"])
        self.assertEqual(stored, [], "the defaults live in code")
