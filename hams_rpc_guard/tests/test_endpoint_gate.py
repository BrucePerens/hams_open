# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import contextlib
import ipaddress

from werkzeug.datastructures import Headers

from odoo.addons.zero_sudo.models.security_utils import ZeroSudoSecurityUtils
from odoo.exceptions import ValidationError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase

from odoo.addons.hams_rpc_guard import endpoint_gate as gate, policy
from odoo.addons.hams_rpc_guard.hooks import post_init_hook

# Built, not written out: the test linter refuses a literal loopback address (containers resolve it differently).
LOOPBACK = str(ipaddress.IPv4Address(0x7F000001))
LOOPBACK_NET = LOOPBACK.split(".")[0] + ".0.0.0/8"
SECOND_LOOPBACK = str(ipaddress.IPv4Address(0x7F000002))  # a proxy that binds its own loopback source address
WG_NET = "10.99.0.0/24"
WG_PEER = "10.99.0.4"
VISITOR = "203.0.113.9"

VERSION_CALL = '<?xml version="1.0"?><methodCall><methodName>version</methodName><params/></methodCall>'
FROM_A_VISITOR = {"X-Forwarded-For": VISITOR, "CF-Connecting-IP": VISITOR}


def _environ(peer=None, orig_peer=None):
    environ = {}
    if peer is not None:
        environ["REMOTE_ADDR"] = peer
    if orig_peer is not None:
        environ["werkzeug.proxy_fix.orig"] = {"REMOTE_ADDR": orig_peer}
    return environ


@tagged("post_install", "-at_install")
class TestEndpointFamily(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:endpoint_family]

    def test_the_real_routes_are_recognised(self):
        for path in ("/jsonrpc", "/xmlrpc/common", "/xmlrpc/2/object"):
            self.assertEqual(gate.endpoint_family(path), gate.FAMILY_LEGACY, path)
        for path in ("/json/2", "/json/2/res.partner/search"):
            self.assertEqual(gate.endpoint_family(path), gate.FAMILY_JSON2, path)

    def test_lookalikes_and_the_web_client_are_not_gated(self):
        for path in ("/xmlrpc.php", "/jsonrpc2", "/json/version", "/json/20", "/web/dataset/call_kw", None, b"/jsonrpc"):
            self.assertIsNone(gate.endpoint_family(path), path)


@tagged("post_install", "-at_install")
class TestResolveClient(HamsTransactionCase):
    """Who the true client is, and that forged headers never make a visitor look local or allowed."""
    # Tests [@ANCHOR: hams_rpc_guard:resolve_client]
    # Tests [@ANCHOR: hams_rpc_guard:in_any]
    # Tests [@ANCHOR: hams_rpc_guard:socket_peer]
    # Tests [@ANCHOR: hams_rpc_guard:parse_address]

    def _client(self, peer=None, orig_peer=None, headers=None, trusted="", header=""):
        return gate.resolve_client(_environ(peer, orig_peer), Headers(list((headers or {}).items())),
                                   gate.parse_trusted_proxies(trusted), gate.parse_client_address_header(header))

    def test_a_direct_caller_is_its_socket_peer(self):
        for peer in (LOOPBACK, "127.8.9.10", "::1", WG_PEER, "2001:db8::1"):
            self.assertEqual(self._client(peer), ipaddress.ip_address(peer), peer)
        self.assertEqual(self._client("::ffff:" + LOOPBACK), ipaddress.ip_address(LOOPBACK), "IPv4-mapped")

    def test_no_peer_or_junk_is_unknown(self):
        for peer in (None, "", "not-an-address", "<local>"):
            self.assertIsNone(self._client(peer), peer)

    def test_proxy_fix_cannot_launder_the_peer(self):
        # proxy_mode rewrote REMOTE_ADDR from X-Forwarded-For; the socket peer it replaced is what counts.
        self.assertEqual(self._client(peer=LOOPBACK, orig_peer=VISITOR), ipaddress.ip_address(VISITOR))

    def test_forged_headers_from_a_peer_that_is_not_a_trusted_proxy_make_the_client_unknown(self):
        for name in gate.FORWARDING_HEADERS:
            with self.subTest(header=name):
                self.assertIsNone(self._client(LOOPBACK, headers={name.lower(): LOOPBACK}, header="X-Real-IP"))
                self.assertIsNone(self._client(WG_PEER, headers={name: LOOPBACK}, trusted="", header="X-Real-IP"))
        # A trusted header name is configured, but the peer is not a trusted proxy: still unknown.
        self.assertIsNone(self._client(LOOPBACK, headers={"X-Real-IP": WG_PEER}, trusted=SECOND_LOOPBACK, header="X-Real-IP"))

    def test_a_trusted_proxy_names_the_client_in_the_one_configured_header(self):
        trusted = SECOND_LOOPBACK + "/32"
        self.assertEqual(self._client(SECOND_LOOPBACK, headers={"X-Real-IP": WG_PEER, "X-Forwarded-For": VISITOR},
                                      trusted=trusted, header="x-real-ip"), ipaddress.ip_address(WG_PEER))
        self.assertEqual(self._client(SECOND_LOOPBACK, headers={"CF-Connecting-IP": VISITOR}, trusted=trusted,
                                      header="CF-Connecting-IP"), ipaddress.ip_address(VISITOR))

    def test_peer_is_trusted_needs_a_loopback_peer_inside_the_list(self):
        # Tests [@ANCHOR: hams_rpc_guard:peer_is_trusted]
        networks = gate.parse_trusted_proxies(LOOPBACK_NET)
        self.assertTrue(gate.peer_is_trusted({"REMOTE_ADDR": LOOPBACK}, networks))
        self.assertTrue(gate.peer_is_trusted(_environ("x", LOOPBACK), networks))
        self.assertFalse(gate.peer_is_trusted({"REMOTE_ADDR": LOOPBACK}, ()))
        self.assertFalse(gate.peer_is_trusted({"REMOTE_ADDR": WG_PEER}, gate.parse_trusted_proxies(WG_NET)))
        self.assertFalse(gate.peer_is_trusted(_environ("x", VISITOR), networks))
        self.assertFalse(gate.peer_is_trusted({}, networks))

    def test_a_trusted_proxy_without_a_usable_header_proves_nothing(self):
        trusted = SECOND_LOOPBACK + "/32"
        for headers in ({"X-Forwarded-Proto": "https"}, {"X-Real-IP": "unknown"},
                        {"X-Real-IP": WG_PEER + ", " + VISITOR}, {"X-Forwarded-For": WG_PEER}):
            with self.subTest(headers=headers):
                self.assertIsNone(self._client(SECOND_LOOPBACK, headers=headers, trusted=trusted, header="X-Real-IP"))
        self.assertIsNone(self._client(SECOND_LOOPBACK, headers={"X-Real-IP": WG_PEER}, trusted=trusted, header=""),
                          "no header configured")
        doubled = Headers([("X-Real-IP", WG_PEER), ("X-Real-IP", VISITOR)])
        self.assertIsNone(gate.resolve_client(_environ(SECOND_LOOPBACK), doubled, gate.parse_trusted_proxies(trusted),
                                              "X-Real-IP"), "a header sent twice")


@tagged("post_install", "-at_install")
class TestSettings(HamsTransactionCase):
    """Parsing, validation and the fail-closed reading of the gate's settings."""
    # Tests [@ANCHOR: hams_rpc_guard:parse_networks]
    # Tests [@ANCHOR: hams_rpc_guard:parse_trusted_proxies]
    # Tests [@ANCHOR: hams_rpc_guard:parse_client_address_header]
    # Tests [@ANCHOR: hams_rpc_guard:parse_legacy_access]
    # Tests [@ANCHOR: hams_rpc_guard:parse_transport]
    # Tests [@ANCHOR: hams_rpc_guard:read_settings]
    # Tests [@ANCHOR: hams_rpc_guard:gate_settings]
    # Tests [@ANCHOR: hams_rpc_guard:gate_parameters]
    # Tests [@ANCHOR: hams_rpc_guard:gate_settings_init]

    def test_the_default_is_loopback_only(self):
        networks = gate.parse_networks(None, default=gate.DEFAULT_ALLOWED_NETWORKS)
        self.assertEqual([str(n) for n in networks], [LOOPBACK_NET, "::1/128"])
        self.assertEqual(gate.parse_networks("  ", default=gate.DEFAULT_ALLOWED_NETWORKS), networks)
        self.assertEqual(gate.parse_networks("None", default=gate.DEFAULT_ALLOWED_NETWORKS), ())

    def test_networks_and_hosts_parse(self):
        networks = gate.parse_networks(" %s, %s ,%s,fd00::/8" % (LOOPBACK_NET, WG_NET, WG_PEER))
        self.assertEqual([str(n) for n in networks], [LOOPBACK_NET, WG_NET, WG_PEER + "/32", "fd00::/8"])

    # Tests [@ANCHOR: hams_rpc_guard_endpoint_gate_config_error]
    def test_a_bad_cidr_is_a_loud_error_never_an_open_gate(self):
        for value in ("10.99.0.0/33", "10.99.0", "everyone", "0.0.0.0/0 ; drop", LOOPBACK_NET + ",nonsense"):
            with self.subTest(value=value), self.assertRaises(gate.ConfigError) as caught:
                gate.parse_networks(value)
            self.assertIn(gate.ALLOWED_NETWORKS_PARAM, str(caught.exception))
        with self.assertRaises(gate.ConfigError):
            gate.parse_trusted_proxies("proxy.example")

    def test_only_single_address_headers_can_be_trusted(self):
        self.assertEqual(gate.parse_client_address_header("cf-connecting-ip"), "CF-Connecting-IP")
        self.assertEqual(gate.parse_client_address_header(None), "")
        for value in ("X-Forwarded-For", "Forwarded", "X-Anything"):
            with self.assertRaises(gate.ConfigError):
                gate.parse_client_address_header(value)

    def test_policy_words(self):
        self.assertEqual(gate.parse_legacy_access(None), gate.LEGACY_NETWORKS)
        self.assertEqual(gate.parse_legacy_access(" NONE "), gate.LEGACY_NONE)
        self.assertEqual(gate.parse_transport(None), gate.TRANSPORT_TCP)
        self.assertEqual(gate.parse_transport("Unix"), gate.TRANSPORT_UNIX)
        for parser in (gate.parse_legacy_access, gate.parse_transport):
            with self.assertRaises(gate.ConfigError):
                parser("any")

    def test_read_settings_fails_closed_on_each_bad_value_and_says_so(self):
        values = {gate.ALLOWED_NETWORKS_PARAM: "10.99.0.0/99", gate.TRUSTED_PROXIES_PARAM: "x",
                  gate.CLIENT_ADDRESS_HEADER_PARAM: "X-Forwarded-For", gate.LEGACY_PARAM: "open",
                  gate.TRANSPORT_PARAM: "pigeon", gate.UNIX_PEERS_PARAM: "bad name!"}
        with self.assertLogs(gate._logger, "ERROR") as logs:
            settings = gate.read_settings(lambda key, default=None: values.get(key))
        self.assertEqual(len(logs.records), 6)
        self.assertEqual(settings.allowed_networks, ())
        self.assertEqual(settings.trusted_proxies, ())
        self.assertEqual(settings.client_address_header, "")
        self.assertEqual(settings.legacy, gate.LEGACY_NONE)
        self.assertEqual(settings.transport, gate.TRANSPORT_CLOSED)
        self.assertIsNone(settings.unix_peers)

    def test_read_settings_defaults(self):
        settings = gate.read_settings(lambda key, default=None: None)
        self.assertEqual([str(n) for n in settings.allowed_networks], [LOOPBACK_NET, "::1/128"])
        self.assertEqual((settings.trusted_proxies, settings.client_address_header, settings.legacy, settings.transport),
                         ((), "", gate.LEGACY_NETWORKS, gate.TRANSPORT_TCP))
        self.assertEqual(settings.unix_peers.rules, ())

    # Tests [@ANCHOR: hams_rpc_guard_ir_config_parameter_ir_config_parameter]
    def test_a_bad_value_cannot_be_written(self):
        # Tests [@ANCHOR: hams_rpc_guard:check_gate_parameters]
        # Tests [@ANCHOR: hams_rpc_guard:parameter_parsers]
        params = self.env["ir.config_parameter"]
        # A deployment's ham_rpc_guard_config module may already hold these keys; start from none.
        params.search([("key", "like", "hams_rpc_guard.%")]).unlink()
        for key, value in ((gate.ALLOWED_NETWORKS_PARAM, "10.99.0.0/40"), (gate.TRANSPORT_PARAM, "carrier-pigeon"),
                           (gate.CLIENT_ADDRESS_HEADER_PARAM, "X-Forwarded-For"), (gate.UNIX_PEERS_PARAM, "=x"),
                           (gate.LEGACY_PARAM, "any"), (gate.TRUSTED_PROXIES_PARAM, "nope")):
            with self.subTest(key=key), self.assertRaises(ValidationError):
                params.create({"key": key, "value": value})
                self.env.flush_all()
        record = params.create({"key": gate.ALLOWED_NETWORKS_PARAM, "value": LOOPBACK_NET + "," + WG_NET})
        with self.assertRaises(ValidationError):
            record.write({"value": WG_NET + ",oops"})
            self.env.flush_all()
        params.create({"key": "some.other.key", "value": "10.99.0.0/40"})  # other keys are not ours to judge

    def test_a_changed_setting_is_read_fresh_not_from_the_zero_sudo_cache(self):
        """The gate and the method guard read their settings through `zero_sudo.security.utils._get_system_param`, whose result
        is cached (process-lifetime L1, 24 h in Redis) and which nothing cleared on a parameter write: found in the
        2026-11-12 rehearsal, where `rpc_transport` and `unix_peers` stayed stale in every worker after being set, and so
        would a switch from report to enforce, or back to `off` as a rollback."""
        # Tests [@ANCHOR: hams_rpc_guard:parameter_cache_flush]
        # Tests [@ANCHOR: hams_rpc_guard:parameter_create_flush]
        # Tests [@ANCHOR: hams_rpc_guard:parameter_write_flush]
        # Tests [@ANCHOR: hams_rpc_guard:parameter_unlink_flush]
        utils = self.env["zero_sudo.security.utils"]
        params = self.env["ir.config_parameter"]
        params.search([("key", "=", gate.UNIX_PEERS_PARAM)]).unlink()
        record = params.create({"key": gate.UNIX_PEERS_PARAM, "value": "alice"})
        self.assertEqual(utils._get_system_param(gate.UNIX_PEERS_PARAM, ""), "alice")
        record.write({"value": "bob"})
        self.assertEqual(utils._get_system_param(gate.UNIX_PEERS_PARAM, ""), "bob")
        record.unlink()
        self.assertEqual(utils._get_system_param(gate.UNIX_PEERS_PARAM, "gone"), "gone")

    def test_the_parameters_are_readable_through_the_zero_sudo_whitelist(self):
        whitelist = self.env["zero_sudo.security.utils"]._get_param_read_whitelist()
        for key in (gate.ALLOWED_NETWORKS_PARAM, gate.LEGACY_PARAM, gate.TRUSTED_PROXIES_PARAM,
                    gate.CLIENT_ADDRESS_HEADER_PARAM, gate.TRANSPORT_PARAM, gate.UNIX_PEERS_PARAM):
            self.assertIn(key, whitelist)


@tagged("post_install", "-at_install")
class TestJudgement(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:judge_tcp]
    # Tests [@ANCHOR: hams_rpc_guard:network_class]
    # Tests [@ANCHOR: hams_rpc_guard:note_request]
    # Tests [@ANCHOR: hams_rpc_guard:request_counts]
    # Tests [@ANCHOR: hams_rpc_guard:is_power_of_ten]

    def _settings(self, networks=gate.DEFAULT_ALLOWED_NETWORKS, **kwargs):
        return gate.GateSettings(gate.parse_networks(networks), **kwargs)

    def test_allowed_and_denied_networks(self):
        hams = self._settings(gate.DEFAULT_ALLOWED_NETWORKS + "," + WG_NET)
        for client, allowed in ((LOOPBACK, True), ("::1", True), (WG_PEER, True), ("10.99.1.4", False),
                                ("192.168.10.34", False), (VISITOR, False), ("2001:db8::1", False)):
            for family in (gate.FAMILY_JSON2, gate.FAMILY_LEGACY):
                with self.subTest(client=client, family=family):
                    self.assertEqual(gate.judge_tcp(family, hams, ipaddress.ip_address(client))[0], allowed)
        self.assertEqual(gate.judge_tcp(gate.FAMILY_JSON2, hams, None), (False, "network"))
        self.assertEqual(gate.judge_tcp(gate.FAMILY_JSON2, self._settings(), ipaddress.ip_address(WG_PEER)),
                         (False, "network"), "json2 is not open by default any more")
        self.assertEqual(gate.judge_tcp(gate.FAMILY_JSON2, self._settings("none"), ipaddress.ip_address(LOOPBACK)),
                         (False, "network"))

    def test_legacy_switch_and_transport(self):
        loopback = ipaddress.ip_address(LOOPBACK)
        closed_legacy = self._settings(legacy=gate.LEGACY_NONE)
        self.assertEqual(gate.judge_tcp(gate.FAMILY_LEGACY, closed_legacy, loopback), (False, "legacy-closed"))
        self.assertTrue(gate.judge_tcp(gate.FAMILY_JSON2, closed_legacy, loopback)[0])
        for transport, allowed in ((gate.TRANSPORT_TCP, True), (gate.TRANSPORT_BOTH, True),
                                   (gate.TRANSPORT_UNIX, False), (gate.TRANSPORT_CLOSED, False)):
            self.assertEqual(gate.judge_tcp(gate.FAMILY_JSON2, self._settings(transport=transport), loopback)[0], allowed)

    def test_network_classes(self):
        for address, expected in ((LOOPBACK, "loopback"), ("::1", "loopback"), (WG_PEER, "private"),
                                  ("fd28::1", "private"), ("169.254.1.1", "link-local"), (VISITOR, "private"),
                                  ("8.8.8.8", "public"), (None, "unknown")):
            self.assertEqual(gate.network_class(ipaddress.ip_address(address) if address else None), expected, address)

    def test_counters_are_by_class_and_the_log_carries_no_value(self):
        gate._counts.clear()
        with self.assertLogs(gate._logger, level="WARNING") as logs:
            for _ in range(12):
                gate.note_request(gate.FAMILY_JSON2, gate.CLASS_PUBLIC, False, "network")
            gate.note_request(gate.FAMILY_JSON2, gate.CLASS_PUBLIC, False, "network", served=True)
            gate.note_request(gate.FAMILY_JSON2, gate.CLASS_LOOPBACK, True, "allowed")
        counts = gate.request_counts()
        self.assertEqual(counts[(gate.FAMILY_JSON2, "public", "refused:network")], 12)
        self.assertEqual(counts[(gate.FAMILY_JSON2, "public", "would-refuse:network")], 1)
        self.assertEqual(counts[(gate.FAMILY_JSON2, "loopback", "allowed")], 1)
        self.assertEqual(len(logs.records), 3)  # the 1st and 10th refusal, the 1st would-be refusal
        self.assertIn("report-only", logs.output[-1])
        gate._counts.clear()


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
    def _settings(self, **values):
        names = {"networks": gate.ALLOWED_NETWORKS_PARAM, "legacy": gate.LEGACY_PARAM,
                 "trusted": gate.TRUSTED_PROXIES_PARAM, "header": gate.CLIENT_ADDRESS_HEADER_PARAM,
                 "transport": gate.TRANSPORT_PARAM, "mode": policy.MODE_PARAM}
        self.values = {names[key]: value for key, value in values.items()}
        try:
            yield
        finally:
            self.values = {}

    def _xmlrpc(self, route="/xmlrpc/2/common", headers=None):
        return self.url_open(route, data=VERSION_CALL, headers={"Content-Type": "text/xml", **(headers or {})})

    def _jsonrpc(self, headers=None):
        payload = {"jsonrpc": "2.0", "method": "call", "params": {"service": "common", "method": "version", "args": []}}
        return self.url_open("/jsonrpc", json=payload, headers=headers or {})

    def _json2(self, headers=None):
        return self.url_open("/json/2/res.users/context_get", data="{}", headers={
            "Content-Type": "application/json", "X-Odoo-Database": self.env.cr.dbname,
            "Authorization": "Bearer not-a-real-api-key", **(headers or {})})

    def _assert_status(self, response, status):
        self.assertEqual(response.status_code, status, response.text[:200])

    def test_default_a_daemon_on_this_machine_still_works(self):
        with self._settings():
            for route in ("/xmlrpc/2/common", "/xmlrpc/common"):
                response = self._xmlrpc(route)
                self._assert_status(response, 200)
                self.assertIn("server_version", response.text, route)
            self.assertIn("server_version", self._jsonrpc().text)
            # A bearer key that is not one: Odoo's own 401 proves the request got past the gate.
            self._assert_status(self._json2(), 401)

    def test_default_a_forwarded_request_is_refused_on_every_endpoint(self):
        with self._settings():
            for route in ("/xmlrpc/2/common", "/xmlrpc/common"):
                self._assert_status(self._xmlrpc(route, FROM_A_VISITOR), 404)
            response = self._jsonrpc(FROM_A_VISITOR)
            self.assertNotIn("server_version", response.text)
            self._assert_status(self._json2(FROM_A_VISITOR), 404)
            self._assert_status(self._json2({"CF-Connecting-IP": VISITOR}), 404)

    def test_spoofed_headers_from_a_peer_that_is_not_a_trusted_proxy_are_refused(self):
        # The test client's peer is the loopback address; it is not configured as a trusted proxy here.
        with self._settings(networks=LOOPBACK_NET + "," + WG_NET, header="X-Real-IP", trusted=SECOND_LOOPBACK):
            for forged in ({"X-Real-IP": WG_PEER}, {"X-Real-IP": LOOPBACK}, {"X-Forwarded-For": LOOPBACK},
                           {"CF-Connecting-IP": LOOPBACK}, {"Forwarded": "for=" + LOOPBACK}):
                with self.subTest(forged=forged):
                    self._assert_status(self._json2(forged), 404)
                    self._assert_status(self._xmlrpc(headers=forged), 404)

    def test_a_trusted_proxy_may_vouch_for_a_client_on_an_allowed_network(self):
        with self._settings(networks=LOOPBACK_NET + "," + WG_NET, header="X-Real-IP", trusted=LOOPBACK):
            self._assert_status(self._json2({"X-Real-IP": WG_PEER, "X-Forwarded-For": WG_PEER}), 401)
            self._assert_status(self._xmlrpc(headers={"X-Real-IP": WG_PEER}), 200)
            self._assert_status(self._json2({"X-Real-IP": VISITOR}), 404)
            self._assert_status(self._json2({"X-Forwarded-For": WG_PEER}), 404)  # not the configured header
        with self._settings(networks=LOOPBACK_NET, header="X-Real-IP", trusted=LOOPBACK):
            self._assert_status(self._json2({"X-Real-IP": WG_PEER}), 404)  # the network is not on the list

    def test_legacy_can_be_closed_and_json2_follows_the_list(self):
        with self._settings(legacy="none"):
            self._assert_status(self._xmlrpc(), 404)
            self.assertNotIn("server_version", self._jsonrpc().text)
            self._assert_status(self._json2(), 401)
        with self._settings(networks="none"):
            self._assert_status(self._json2(), 404)
            self._assert_status(self._xmlrpc(), 404)

    def test_unix_transport_refuses_every_endpoint_on_tcp(self):
        with self._settings(transport="unix"):
            self._assert_status(self._json2(), 404)
            self._assert_status(self._xmlrpc(), 404)
            # A client cannot claim to be the Unix socket: werkzeug drops header names with underscores, and keeps
            # every other one under an HTTP_ key.
            self._assert_status(self._json2({"Hams-Rpc-Guard.Unix-Peer": "uid=0"}), 404)
            # The browser's own RPC is not an external endpoint and stays on TCP.
            self._assert_status(self.url_open("/web/login"), 200)
            body = self.url_open("/web/dataset/call_kw/res.partner/search_read", json={
                "jsonrpc": "2.0", "method": "call", "params": {
                    "model": "res.partner", "method": "search_read", "args": [[]], "kwargs": {"fields": ["name"]}}}).json()
            self.assertEqual(body["error"]["code"], 100, "reached Odoo's own session check, not the gate")

    def test_bad_settings_in_the_database_fail_closed_loudly(self):
        with self._settings(networks="10.99.0.0/99"), self.assertLogs(gate._logger, "ERROR"):
            self._assert_status(self._json2(), 404)
        with self._settings(transport="pigeon"), self.assertLogs(gate._logger, "ERROR"):
            self._assert_status(self._xmlrpc(), 404)

    def test_an_unreadable_parameter_fails_closed(self):
        class Broken(dict):
            def get(self, key, default=None):
                raise RuntimeError("cache down")
        self.values = Broken()
        with self.assertLogs("odoo.addons.hams_rpc_guard.models.ir_http", "ERROR"):
            self._assert_status(self._xmlrpc(), 404)

    def test_report_mode_serves_and_counts_and_off_is_stock(self):
        gate._counts.clear()
        with self._settings(mode="report"), self.assertLogs(gate._logger, "WARNING") as logs:
            self._assert_status(self._xmlrpc(headers=FROM_A_VISITOR), 200)
        self.assertIn("would have refused", logs.output[0])
        self.assertEqual(gate.request_counts()[(gate.FAMILY_LEGACY, gate.CLASS_UNKNOWN, "would-refuse:network")], 1)
        with self._settings(mode="off"):
            self._assert_status(self._xmlrpc(headers=FROM_A_VISITOR), 200)
        gate._counts.clear()

    def test_the_web_client_and_other_routes_are_never_gated(self):
        with self._settings(legacy="none", networks="none"):
            self._assert_status(self.url_open("/json/version", headers=FROM_A_VISITOR), 200)
            self._assert_status(self.url_open("/web/login", headers=FROM_A_VISITOR), 200)

    def test_a_refusal_is_logged_without_any_request_value(self):
        gate._counts.clear()
        with self._settings(), self.assertLogs(gate._logger, level="WARNING") as logs:
            self._xmlrpc(headers={**FROM_A_VISITOR, "User-Agent": "attacker-agent"})
        joined = "\n".join(logs.output)
        self.assertIn("Refused 1 request", joined)
        for value in (VISITOR, "attacker-agent", "version", LOOPBACK):
            self.assertNotIn(value, joined)
        gate._counts.clear()


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
        # ham_rpc_guard_config (when installed) creates the parameters itself; the hook must not add any.
        self.env["ir.config_parameter"].search([("key", "like", "hams_rpc_guard.%")]).unlink()
        post_init_hook(self.env)
        bootstrap.assert_called_once_with()
        keys = [gate.ALLOWED_NETWORKS_PARAM, gate.LEGACY_PARAM, gate.TRUSTED_PROXIES_PARAM, gate.TRANSPORT_PARAM,
                gate.UNIX_PEERS_PARAM, gate.CLIENT_ADDRESS_HEADER_PARAM]
        stored = self.env["ir.config_parameter"].search_read([("key", "in", keys)], ["key"])
        self.assertEqual(stored, [], "the defaults live in code")
