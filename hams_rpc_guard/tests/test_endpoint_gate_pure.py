# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Direct tests of the pure functions of endpoint_gate.py, added to kill the mutants the HTTP-level tests left alive
(mutation run 2026-11-13). No database and no request: every rule is pinned with its exact verdict."""
import ipaddress
import logging

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from werkzeug.datastructures import Headers

from odoo.addons.hams_rpc_guard import endpoint_gate as gate
from odoo.addons.hams_rpc_guard.endpoint_gate import ConfigError, GateSettings, Peer


def _net(*texts):
    return tuple(ipaddress.ip_network(text) for text in texts)


def _addr(text):
    return ipaddress.ip_address(text)


@tagged("post_install", "-at_install")
class TestGatePure(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:endpoint_family]
    # Tests [@ANCHOR: hams_rpc_guard:parse_address]
    # Tests [@ANCHOR: hams_rpc_guard:parse_networks]
    # Tests [@ANCHOR: hams_rpc_guard:parse_trusted_proxies]
    # Tests [@ANCHOR: hams_rpc_guard:parse_client_address_header]
    # Tests [@ANCHOR: hams_rpc_guard:parse_legacy_access]
    # Tests [@ANCHOR: hams_rpc_guard:parse_transport]
    # Tests [@ANCHOR: hams_rpc_guard:peer]
    # Tests [@ANCHOR: hams_rpc_guard:peer_init]
    # Tests [@ANCHOR: hams_rpc_guard:peer_as_dict]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rule]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rule_init]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rule_matches]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rules]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rules_init]
    # Tests [@ANCHOR: hams_rpc_guard:peer_rules_match]
    # Tests [@ANCHOR: hams_rpc_guard:parse_peer_rules]
    # Tests [@ANCHOR: hams_rpc_guard:gate_settings]
    # Tests [@ANCHOR: hams_rpc_guard:gate_settings_init]
    # Tests [@ANCHOR: hams_rpc_guard:read_settings]
    # Tests [@ANCHOR: hams_rpc_guard:socket_peer]
    # Tests [@ANCHOR: hams_rpc_guard:resolve_client]
    # Tests [@ANCHOR: hams_rpc_guard:peer_is_trusted]
    # Tests [@ANCHOR: hams_rpc_guard:in_any]
    # Tests [@ANCHOR: hams_rpc_guard:network_class]
    # Tests [@ANCHOR: hams_rpc_guard:judge_tcp]
    # Tests [@ANCHOR: hams_rpc_guard:judge_unix]
    # Tests [@ANCHOR: hams_rpc_guard:note_request]
    # Tests [@ANCHOR: hams_rpc_guard:request_counts]
    # Tests [@ANCHOR: hams_rpc_guard:is_power_of_ten]

    def setUp(self):
        super().setUp()
        gate._counts.clear()
        self.addCleanup(gate._counts.clear)

    # -- endpoint_family

    def test_endpoint_family_is_exact(self):
        fam = gate.endpoint_family
        self.assertEqual(fam("/jsonrpc"), gate.FAMILY_LEGACY)
        self.assertEqual(fam("/xmlrpc/2/object"), gate.FAMILY_LEGACY)
        self.assertEqual(fam("/json/2"), gate.FAMILY_JSON2)
        self.assertEqual(fam("/json/2/res.partner/search"), gate.FAMILY_JSON2)
        for path in ("/xmlrpc.php", "/xmlrpc", "/jsonrpcx", "/json/20", "/json/", "/web", "", "/json/2x", None, b"/jsonrpc", 7):
            self.assertIsNone(fam(path), repr(path))

    # -- _parse_address

    def test_parse_address(self):
        parse = gate._parse_address
        self.assertEqual(parse("192.0.2.7"), _addr("192.0.2.7"))
        self.assertEqual(parse("  192.0.2.7 "), _addr("192.0.2.7"))
        self.assertEqual(parse('"192.0.2.7"'), _addr("192.0.2.7"))
        self.assertEqual(parse("192.0.2.7:8069"), _addr("192.0.2.7"), "host:port")
        self.assertEqual(parse("[2001:db8::1]"), _addr("2001:db8::1"))
        self.assertEqual(parse("[2001:db8::1]:443"), _addr("2001:db8::1"), "bracketed with a port")
        self.assertEqual(parse("2001:db8::1"), _addr("2001:db8::1"), "an IPv6 address has many colons, not a port")
        self.assertEqual(parse("fe80::1%eth0"), _addr("fe80::1"), "zone id dropped")
        self.assertEqual(parse("192.0.2.7%eth0"), _addr("192.0.2.7"))
        mapped = parse("::ffff:192.0.2.7")
        self.assertEqual(mapped, _addr("192.0.2.7"))
        self.assertEqual(mapped.version, 4, "an IPv4-mapped address is the IPv4 one")
        self.assertEqual(parse("2001:db8::1").version, 6)
        self.assertEqual(parse(_addr("192.0.2.9")), _addr("192.0.2.9"))
        for bad in ("", "nonsense", "300.1.1.1", "[", "[]", "1.2.3", "host:80", "a:b:c"):
            self.assertIsNone(parse(bad), bad)
        self.assertIsNone(parse("[2001:db8::1"), "an unclosed bracket is not an address")

    # -- parse_networks and the other parsers

    def test_parse_networks(self):
        self.assertEqual(gate.parse_networks("10.0.0.0/8, 192.0.2.5 ,,::1"),
                         _net("10.0.0.0/8", "192.0.2.5/32", "::1/128"))
        self.assertEqual(gate.parse_networks("10.0.0.5/8"), _net("10.0.0.0/8"), "host bits are not an error")
        self.assertEqual(gate.parse_networks(None, default="127.0.0.0/8"), _net("127.0.0.0/8"))
        self.assertEqual(gate.parse_networks(False, default="127.0.0.0/8"), _net("127.0.0.0/8"))
        self.assertEqual(gate.parse_networks("   ", default="::1"), _net("::1/128"))
        self.assertEqual(gate.parse_networks("", default=""), ())
        self.assertEqual(gate.parse_networks(None), (), "the default default is no network")
        self.assertEqual(gate.parse_networks("none", default="127.0.0.0/8"), ())
        self.assertEqual(gate.parse_networks(" NONE "), ())
        self.assertEqual(gate.parse_networks(None, default="none"), (), "the word applies to the default too")
        self.assertEqual(gate.parse_networks(","), ())
        with self.assertRaises(ConfigError) as caught:
            gate.parse_networks("10.0.0.0/8,bogus", param="some.param")
        self.assertEqual(str(caught.exception),
                         "some.param: 'bogus' is not an IPv4 or IPv6 address or network (CIDR).")
        self.assertIsNone(caught.exception.__cause__)
        self.assertTrue(caught.exception.__suppress_context__, "raised from None")
        with self.assertRaises(ConfigError) as caught:
            gate.parse_networks("bogus")
        self.assertIn(gate.ALLOWED_NETWORKS_PARAM, str(caught.exception), "the default parameter name")
        for junk in ("10.0.0.0/33", "1.2.3", "::g"):
            with self.assertRaises(ConfigError):
                gate.parse_networks(junk)

    def test_parse_trusted_proxies(self):
        self.assertEqual(gate.parse_trusted_proxies(None), ())
        self.assertEqual(gate.parse_trusted_proxies("127.0.0.2"), _net("127.0.0.2/32"))
        self.assertEqual(gate.parse_trusted_proxies("none"), ())
        with self.assertRaises(ConfigError) as caught:
            gate.parse_trusted_proxies("x")
        self.assertIn(gate.TRUSTED_PROXIES_PARAM, str(caught.exception))

    def test_parse_client_address_header(self):
        parse = gate.parse_client_address_header
        for blank in (None, False, "", "  "):
            self.assertEqual(parse(blank), "", repr(blank))
        self.assertEqual(parse("cf-connecting-ip"), "CF-Connecting-IP")
        self.assertEqual(parse(" X-REAL-IP "), "X-Real-IP")
        self.assertEqual(parse("True-Client-IP"), "True-Client-IP")
        with self.assertRaises(ConfigError) as caught:
            parse("X-Forwarded-For")
        self.assertEqual(str(caught.exception), "hams_rpc_guard.client_address_header: 'X-Forwarded-For' is not one of "
                                                "CF-Connecting-IP, X-Real-IP, True-Client-IP.")

    def test_parse_legacy_access(self):
        parse = gate.parse_legacy_access
        for blank in (None, False, "", " "):
            self.assertEqual(parse(blank), gate.LEGACY_NETWORKS, repr(blank))
        self.assertEqual(parse(" Networks "), gate.LEGACY_NETWORKS)
        self.assertEqual(parse("NONE"), gate.LEGACY_NONE)
        with self.assertRaises(ConfigError) as caught:
            parse("open")
        self.assertEqual(str(caught.exception), "hams_rpc_guard.legacy_access: 'open' is not 'networks' or 'none'.")

    def test_parse_transport(self):
        parse = gate.parse_transport
        for blank in (None, False, "", " "):
            self.assertEqual(parse(blank), gate.TRANSPORT_TCP, repr(blank))
        self.assertEqual(parse(" UNIX "), gate.TRANSPORT_UNIX)
        self.assertEqual(parse("Both"), gate.TRANSPORT_BOTH)
        self.assertEqual(parse("tcp"), gate.TRANSPORT_TCP)
        with self.assertRaises(ConfigError) as caught:
            parse("closed")
        self.assertEqual(str(caught.exception),
                         "hams_rpc_guard.rpc_transport: 'closed' is not one of tcp, unix, both.",
                         "'closed' is what a bad value is read as, never a value an operator may write")

    # -- Peer, PeerRule, PeerRules

    def test_peer_and_rules(self):
        peer = Peer(10, 1000, 100, "alice", "staff")
        self.assertEqual(peer.as_dict(), {"pid": 10, "uid": 1000, "gid": 100, "user": "alice", "group": "staff"})
        bare = Peer(1, 2, 3)
        self.assertEqual((bare.user, bare.group), ("", ""))
        for kind, value, expected in (("user", "alice", True), ("user", "bob", False), ("uid", 1000, True),
                                      ("uid", 100, False), ("group", "staff", True), ("group", "alice", False),
                                      ("gid", 100, True), ("gid", 1000, False)):
            self.assertEqual(gate.PeerRule(kind, value).matches(peer), expected, (kind, value))
        rule = gate.PeerRule("user", "alice", ["a", "b", "a"])
        self.assertEqual(rule.logins, frozenset({"a", "b"}))
        self.assertEqual(gate.PeerRule("user", "alice").logins, frozenset())
        first, second = gate.PeerRule("user", "alice", ["x"]), gate.PeerRule("uid", 1000, ["y"])
        self.assertIs(gate.PeerRules([first, second]).match(peer), first)
        self.assertIs(gate.PeerRules([gate.PeerRule("user", "bob"), second]).match(peer), second)
        self.assertIsNone(gate.PeerRules([gate.PeerRule("user", "bob")]).match(peer))
        self.assertIsNone(gate.PeerRules().match(peer))
        self.assertEqual(gate.PeerRules().rules, ())

    def test_parse_peer_rules_kinds(self):
        rules = gate.parse_peer_rules(" alice , 1001 , @staff , @200 ,, svc$ ").rules
        self.assertEqual([(r.kind, r.value, r.logins) for r in rules],
                         [("user", "alice", frozenset()), ("uid", 1001, frozenset()), ("group", "staff", frozenset()),
                          ("gid", 200, frozenset()), ("user", "svc$", frozenset())])
        self.assertEqual(gate.parse_peer_rules(None).rules, ())
        self.assertEqual(gate.parse_peer_rules(False).rules, ())
        self.assertEqual(gate.parse_peer_rules("  ").rules, ())
        self.assertEqual(gate.parse_peer_rules(",,").rules, ())
        self.assertEqual(gate.parse_peer_rules("a_b.c-d").rules[0].value, "a_b.c-d")

    def test_parse_peer_rules_logins(self):
        rules = gate.parse_peer_rules("alice = one | two ,@grp=three, 7=x").rules
        self.assertEqual(rules[0].logins, frozenset({"one", "two"}))
        self.assertEqual((rules[0].kind, rules[0].value), ("user", "alice"))
        self.assertEqual((rules[1].kind, rules[1].value, rules[1].logins), ("group", "grp", frozenset({"three"})))
        self.assertEqual((rules[2].kind, rules[2].value, rules[2].logins), ("uid", 7, frozenset({"x"})))
        for bad in ("alice=", "alice=a|", "alice=|a", "alice=a b", "alice=a,b=", "alice=a=b", "alice= "):
            with self.assertRaises(ConfigError, msg=bad) as caught:
                gate.parse_peer_rules(bad)
            self.assertTrue(str(caught.exception).startswith("hams_rpc_guard.unix_peers: "), bad)
        with self.assertRaises(ConfigError) as caught:
            gate.parse_peer_rules("good,alice=")
        self.assertEqual(str(caught.exception), "hams_rpc_guard.unix_peers: 'alice=' has an empty or malformed login list.")

    def test_parse_peer_rules_rejects_malformed_names(self):
        for bad in ("1abc", "-x", "a b", "@", "@@x", "a/b", "²", "@²", "٣", "al$ice", "a,b c"):
            with self.assertRaises(ConfigError, msg=bad):
                gate.parse_peer_rules(bad)
        with self.assertRaises(ConfigError) as caught:
            gate.parse_peer_rules("1abc")
        self.assertEqual(str(caught.exception),
                         "hams_rpc_guard.unix_peers: '1abc' is not a user name, uid, @group or @gid.")

    # -- read_settings

    def test_read_settings_defaults_and_values(self):
        values = {}
        settings = gate.read_settings(lambda key, default: values.get(key, default))
        self.assertEqual(settings.allowed_networks, _net("127.0.0.0/8", "::1/128"))
        self.assertEqual((settings.trusted_proxies, settings.client_address_header), ((), ""))
        self.assertEqual((settings.legacy, settings.transport), ("networks", "tcp"))
        self.assertEqual(settings.unix_peers.rules, ())
        values.update({
            gate.ALLOWED_NETWORKS_PARAM: "10.0.0.0/8", gate.TRUSTED_PROXIES_PARAM: "127.0.0.2",
            gate.CLIENT_ADDRESS_HEADER_PARAM: "x-real-ip", gate.LEGACY_PARAM: "none", gate.TRANSPORT_PARAM: "both",
            gate.UNIX_PEERS_PARAM: "alice=l"})
        settings = gate.read_settings(lambda key, default: values.get(key, default))
        self.assertEqual(settings.allowed_networks, _net("10.0.0.0/8"))
        self.assertEqual(settings.trusted_proxies, _net("127.0.0.2/32"))
        self.assertEqual((settings.client_address_header, settings.legacy, settings.transport),
                         ("X-Real-IP", "none", "both"))
        self.assertEqual(settings.unix_peers.rules[0].logins, frozenset({"l"}))

    def test_read_settings_fails_closed_per_setting_and_logs_each(self):
        bad = {key: "junk!" for key in (gate.ALLOWED_NETWORKS_PARAM, gate.TRUSTED_PROXIES_PARAM,
                                        gate.CLIENT_ADDRESS_HEADER_PARAM, gate.LEGACY_PARAM, gate.TRANSPORT_PARAM,
                                        gate.UNIX_PEERS_PARAM)}
        with self.assertLogs(gate._logger, "ERROR") as logs:
            settings = gate.read_settings(lambda key, default: bad[key])
        self.assertEqual(len(logs.records), 6)
        self.assertEqual(settings.allowed_networks, ())
        self.assertEqual(settings.trusted_proxies, ())
        self.assertEqual(settings.client_address_header, "")
        self.assertEqual(settings.legacy, gate.LEGACY_NONE)
        self.assertEqual(settings.transport, gate.TRANSPORT_CLOSED)
        self.assertIsNone(settings.unix_peers)
        self.assertTrue(all(record.getMessage().startswith("RPC guard configuration error, failing closed: hams_rpc_guard.")
                            and record.getMessage().endswith(".") for record in logs.records))
        self.assertEqual(logs.records[3].getMessage(),
                         "RPC guard configuration error, failing closed: hams_rpc_guard.legacy_access: "
                         "'junk!' is not 'networks' or 'none'.")
        # one bad setting does not spoil the others
        only = {gate.LEGACY_PARAM: "junk", gate.ALLOWED_NETWORKS_PARAM: "10.0.0.0/8"}
        with self.assertLogs(gate._logger, "ERROR") as logs:
            settings = gate.read_settings(lambda key, default: only.get(key, default))
        self.assertEqual(len(logs.records), 1)
        self.assertEqual(settings.allowed_networks, _net("10.0.0.0/8"))
        self.assertEqual(settings.legacy, gate.LEGACY_NONE)
        self.assertEqual(settings.transport, gate.TRANSPORT_TCP)

    def test_read_settings_passes_none_as_default_and_lets_a_reader_error_propagate(self):
        seen = []
        gate.read_settings(lambda key, default: seen.append((key, default)))
        self.assertEqual(len(seen), 6)
        self.assertTrue(all(default is None for _key, default in seen))

        def boom(key, default):
            raise RuntimeError("db down")
        with self.assertRaises(RuntimeError):
            gate.read_settings(boom)

    def test_gate_settings_defaults_are_closed_where_they_can_be(self):
        settings = GateSettings([ipaddress.ip_network("10.0.0.0/8")])
        self.assertEqual(settings.allowed_networks, _net("10.0.0.0/8"))
        self.assertEqual((settings.trusted_proxies, settings.client_address_header, settings.legacy, settings.transport),
                         ((), "", "networks", "tcp"))
        self.assertIsNone(settings.unix_peers, "no Unix peer admitted unless the rules were read")

    # -- socket_peer, resolve_client, peer_is_trusted

    def test_socket_peer(self):
        self.assertEqual(gate.socket_peer({"REMOTE_ADDR": "192.0.2.1"}), _addr("192.0.2.1"))
        self.assertIsNone(gate.socket_peer({}))
        self.assertIsNone(gate.socket_peer({"REMOTE_ADDR": ""}))
        self.assertIsNone(gate.socket_peer({"REMOTE_ADDR": "not-an-ip"}))
        proxied = {"REMOTE_ADDR": "203.0.113.9", "werkzeug.proxy_fix.orig": {"REMOTE_ADDR": "127.0.0.2"}}
        self.assertEqual(gate.socket_peer(proxied), _addr("127.0.0.2"), "ProxyFix's saved original wins")
        self.assertEqual(gate.socket_peer({"REMOTE_ADDR": "192.0.2.1", "werkzeug.proxy_fix.orig": {}}),
                         _addr("192.0.2.1"))
        self.assertEqual(gate.socket_peer({"REMOTE_ADDR": "192.0.2.1", "werkzeug.proxy_fix.orig": None}),
                         _addr("192.0.2.1"))
        self.assertEqual(gate.socket_peer({"REMOTE_ADDR": "192.0.2.1", "werkzeug.proxy_fix.orig": {"X": "y"}}),
                         _addr("192.0.2.1"), "an original without REMOTE_ADDR falls back to the environ's")
        self.assertIsNone(gate.socket_peer({"werkzeug.proxy_fix.orig": {"REMOTE_ADDR": None}}))

    def test_resolve_client_direct_and_forwarded(self):
        proxies = _net("127.0.0.2/32")
        environ = {"REMOTE_ADDR": "127.0.0.2"}
        self.assertEqual(gate.resolve_client(environ, Headers()), _addr("127.0.0.2"))
        self.assertIsNone(gate.resolve_client({}, Headers()))
        self.assertIsNone(gate.resolve_client({"REMOTE_ADDR": "x"}, Headers([("Via", "1.1 a")]), proxies, "X-Real-IP"))
        # every forwarding header makes a request "proxied"
        for name in gate.FORWARDING_HEADERS:
            self.assertIsNone(gate.resolve_client(environ, Headers([(name, "1.2.3.4")])), name)
            self.assertIsNone(gate.resolve_client(environ, Headers([(name, "1.2.3.4")]), proxies, ""), name)
        # a trusted proxy with a header configured vouches for the client
        headers = Headers([("X-Real-IP", "198.51.100.4")])
        self.assertEqual(gate.resolve_client(environ, headers, proxies, "X-Real-IP"), _addr("198.51.100.4"))
        # not from a trusted proxy
        self.assertIsNone(gate.resolve_client({"REMOTE_ADDR": "192.0.2.1"}, headers, proxies, "X-Real-IP"))
        self.assertIsNone(gate.resolve_client(environ, headers, (), "X-Real-IP"))
        self.assertIsNone(gate.resolve_client(environ, headers, proxies, ""))
        # exactly one value, no list
        two = Headers([("X-Real-IP", "198.51.100.4"), ("X-Real-IP", "198.51.100.5")])
        self.assertIsNone(gate.resolve_client(environ, two, proxies, "X-Real-IP"))
        listed = Headers([("X-Real-IP", "198.51.100.4, 198.51.100.5")])
        self.assertIsNone(gate.resolve_client(environ, listed, proxies, "X-Real-IP"))
        missing = Headers([("Via", "1.1 p")])
        self.assertIsNone(gate.resolve_client(environ, missing, proxies, "X-Real-IP"), "configured header absent")
        junk = Headers([("X-Real-IP", "junk")])
        self.assertIsNone(gate.resolve_client(environ, junk, proxies, "X-Real-IP"))
        # the proxy network is matched per address family
        v6 = {"REMOTE_ADDR": "::1"}
        self.assertEqual(gate.resolve_client(v6, headers, _net("::1/128"), "X-Real-IP"), _addr("198.51.100.4"))
        self.assertIsNone(gate.resolve_client(v6, headers, _net("127.0.0.0/8"), "X-Real-IP"))
        # ProxyFix's original address is the one judged
        fixed = {"REMOTE_ADDR": "198.51.100.4", "werkzeug.proxy_fix.orig": {"REMOTE_ADDR": "127.0.0.2"}}
        self.assertEqual(gate.resolve_client(fixed, headers, proxies, "X-Real-IP"), _addr("198.51.100.4"))

    def test_peer_is_trusted_needs_a_loopback_peer_inside_a_trusted_network(self):
        proxies = _net("127.0.0.0/8", "10.0.0.0/8")
        self.assertTrue(gate.peer_is_trusted({"REMOTE_ADDR": "127.0.0.2"}, proxies))
        self.assertTrue(gate.peer_is_trusted({"REMOTE_ADDR": "::ffff:127.0.0.2"}, proxies))
        self.assertFalse(gate.peer_is_trusted({"REMOTE_ADDR": "10.1.2.3"}, proxies), "trusted but not loopback")
        self.assertFalse(gate.peer_is_trusted({"REMOTE_ADDR": "127.0.0.2"}, _net("10.0.0.0/8")), "loopback but not trusted")
        self.assertFalse(gate.peer_is_trusted({"REMOTE_ADDR": "127.0.0.2"}, ()))
        self.assertFalse(gate.peer_is_trusted({}, proxies))
        self.assertFalse(gate.peer_is_trusted({"REMOTE_ADDR": "junk"}, proxies))
        self.assertIs(gate.peer_is_trusted({}, proxies), False)

    def test_in_any_compares_only_the_same_family(self):
        self.assertTrue(gate._in_any(_addr("10.1.1.1"), _net("192.0.2.0/24", "10.0.0.0/8")))
        self.assertFalse(gate._in_any(_addr("11.1.1.1"), _net("10.0.0.0/8")))
        self.assertFalse(gate._in_any(_addr("10.1.1.1"), ()))
        self.assertTrue(gate._in_any(_addr("::1"), _net("10.0.0.0/8", "::1/128")))
        self.assertFalse(gate._in_any(_addr("::1"), _net("0.0.0.0/0")))
        self.assertFalse(gate._in_any(_addr("10.1.1.1"), _net("::/0")))

    def test_network_class(self):
        for text, expected in (("127.0.0.2", "loopback"), ("::1", "loopback"), ("169.254.1.1", "link-local"),
                               ("fe80::1", "link-local"), ("10.1.1.1", "private"), ("192.168.0.1", "private"),
                               ("fd00::1", "private"), ("8.8.8.8", "public"), ("2606:4700::1", "public")):
            self.assertEqual(gate.network_class(_addr(text)), expected, text)
        self.assertEqual(gate.network_class(None), "unknown")

    # -- judge_tcp, judge_unix

    def test_judge_tcp(self):
        nets = _net("10.0.0.0/8")
        ok = GateSettings(nets, transport="tcp")
        both = GateSettings(nets, transport="both")
        legacy = gate.FAMILY_LEGACY
        json2 = gate.FAMILY_JSON2
        inside, outside = _addr("10.1.1.1"), _addr("11.1.1.1")
        self.assertEqual(gate.judge_tcp(legacy, ok, inside), (True, "allowed"))
        self.assertEqual(gate.judge_tcp(json2, both, inside), (True, "allowed"))
        self.assertEqual(gate.judge_tcp(legacy, ok, outside), (False, "network"))
        self.assertEqual(gate.judge_tcp(legacy, ok, None), (False, "network"))
        self.assertEqual(gate.judge_tcp(legacy, GateSettings((), transport="tcp"), inside), (False, "network"))
        for transport in ("unix", "closed", "junk"):
            self.assertEqual(gate.judge_tcp(legacy, GateSettings(nets, transport=transport), inside),
                             (False, "transport"), transport)
        closed_legacy = GateSettings(nets, legacy="none")
        self.assertEqual(gate.judge_tcp(legacy, closed_legacy, inside), (False, "legacy-closed"))
        self.assertEqual(gate.judge_tcp(json2, closed_legacy, inside), (True, "allowed"), "json2 ignores legacy")
        self.assertEqual(gate.judge_tcp(legacy, closed_legacy, None), (False, "legacy-closed"))
        # the transport is checked before the legacy switch, and the legacy switch before the network
        self.assertEqual(gate.judge_tcp(legacy, GateSettings(nets, legacy="none", transport="unix"), outside),
                         (False, "transport"))
        self.assertEqual(gate.judge_tcp(legacy, closed_legacy, outside), (False, "legacy-closed"))

    def test_judge_unix(self):
        peer = Peer(1, 1000, 100, "alice", "staff")
        rules = gate.parse_peer_rules("alice=l1|l2")
        nothing = frozenset()
        unix = GateSettings((), transport="unix", unix_peers=rules)
        both = GateSettings((), transport="both", unix_peers=rules)
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, unix, peer), (True, "allowed", frozenset({"l1", "l2"})))
        self.assertEqual(gate.judge_unix(gate.FAMILY_LEGACY, both, peer), (True, "allowed", frozenset({"l1", "l2"})))
        for transport in ("tcp", "closed", "junk"):
            settings = GateSettings((), transport=transport, unix_peers=rules)
            self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, settings, peer), (False, "transport", nothing), transport)
        legacy_none = GateSettings((), legacy="none", transport="unix", unix_peers=rules)
        self.assertEqual(gate.judge_unix(gate.FAMILY_LEGACY, legacy_none, peer), (False, "legacy-closed", nothing))
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, legacy_none, peer), (True, "allowed", frozenset({"l1", "l2"})))
        unread = GateSettings((), transport="unix", unix_peers=None)
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, unread, peer), (False, "peer", nothing))
        stranger = Peer(2, 2000, 200, "mallory", "x")
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, unix, stranger), (False, "peer", nothing))
        empty = GateSettings((), transport="unix", unix_peers=gate.PeerRules())
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, empty, peer), (False, "peer", nothing))
        anyone = GateSettings((), transport="unix", unix_peers=gate.parse_peer_rules("alice"))
        self.assertEqual(gate.judge_unix(gate.FAMILY_JSON2, anyone, peer), (True, "allowed", nothing))
        # transport before legacy before peers
        both_bad = GateSettings((), legacy="none", transport="tcp", unix_peers=None)
        self.assertEqual(gate.judge_unix(gate.FAMILY_LEGACY, both_bad, peer), (False, "transport", nothing))
        legacy_unread = GateSettings((), legacy="none", transport="unix", unix_peers=None)
        self.assertEqual(gate.judge_unix(gate.FAMILY_LEGACY, legacy_unread, peer), (False, "legacy-closed", nothing))

    # -- note_request, request_counts, is_power_of_ten

    def test_note_request_counts_by_outcome(self):
        fam, cls = gate.FAMILY_JSON2, "public"
        self.assertEqual(gate.note_request(fam, cls, True, "allowed"), 1)
        self.assertEqual(gate.note_request(fam, cls, True, "allowed"), 2)
        with self.assertLogs(gate._logger, "WARNING"):
            self.assertEqual(gate.note_request(fam, cls, False, "network"), 1)
            self.assertEqual(gate.note_request(fam, cls, False, "network", served=False), 2)
            self.assertEqual(gate.note_request(fam, cls, False, "network", served=True), 1)
        self.assertEqual(gate.request_counts(), {
            (fam, cls, "allowed"): 2, (fam, cls, "refused:network"): 2, (fam, cls, "would-refuse:network"): 1})
        copy = gate.request_counts()
        copy.clear()
        self.assertEqual(len(gate.request_counts()), 3, "request_counts returns a copy")

    def test_note_request_served_defaults_to_allowed(self):
        self.assertEqual(gate.note_request("f", "c", True, "allowed", served=False), 1)
        self.assertEqual(list(gate.request_counts()), [("f", "c", "allowed")], "an allowed request is 'allowed'")

    def test_note_request_logs_at_powers_of_ten_only_for_refusals(self):
        for _ in range(30):
            gate.note_request("f", "c", True, "allowed")  # 10 and 20 would log if an allowed request logged
        with self.assertLogs(gate._logger, "WARNING") as logs:
            for _ in range(100):
                gate.note_request("legacy-rpc", "public", False, "network")
        self.assertEqual([record.args[1] for record in logs.records], [1, 10, 100])
        record = logs.records[0]
        self.assertEqual(record.levelno, logging.WARNING)
        self.assertEqual(record.args, ("Refused", 1, "legacy-rpc", "public", "network"))
        with self.assertLogs(gate._logger, "WARNING") as logs:
            gate.note_request("legacy-rpc", "public", False, "network", served=True)
        self.assertEqual(logs.records[0].args[0], "Served (report-only mode), would have refused,")
        self.assertNotIn("1.2.3.4", logs.output[0])

    def test_is_power_of_ten(self):
        for count in (1, 10, 100, 1000, 10 ** 9):
            self.assertTrue(gate.is_power_of_ten(count), count)
        for count in (0, 2, 9, 11, 20, 90, 101, 110, 200, 1010, 1100, 5000, -10, -1):
            self.assertFalse(gate.is_power_of_ten(count), count)
