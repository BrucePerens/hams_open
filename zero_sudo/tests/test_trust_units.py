# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free tests of the tunnel-peer test (peer_address.py) and of `_get_trusted_client_ip`, added to kill the
mutation survivors (mutation run 2026-11-13): a forwarding header is believed only when the transport peer is loopback."""
import ipaddress
from types import SimpleNamespace

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.zero_sudo.models import security_utils
from odoo.addons.zero_sudo.peer_address import is_loopback_peer, parse_peer_address


def _request(remote, **headers):
    return SimpleNamespace(httprequest=SimpleNamespace(remote_addr=remote, headers=headers))


def _utils():
    return object.__new__(security_utils.ZeroSudoSecurityUtils)


@tagged("post_install", "-at_install")
class TestTunnelPeerUnits(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:parse_peer_address]
    # Tests [@ANCHOR: zero_sudo:is_loopback_peer]

    def test_parse_peer_address(self):
        self.assertEqual(parse_peer_address("192.0.2.7"), ipaddress.IPv4Address("192.0.2.7"))
        self.assertEqual(parse_peer_address("  192.0.2.7 \n"), ipaddress.IPv4Address("192.0.2.7"))
        self.assertEqual(parse_peer_address("2001:db8::1"), ipaddress.IPv6Address("2001:db8::1"))
        mapped = parse_peer_address("::ffff:192.0.2.7")
        self.assertEqual(mapped, ipaddress.IPv4Address("192.0.2.7"))
        self.assertEqual(mapped.version, 4)
        self.assertEqual(parse_peer_address("::ffff:7f00:1"), ipaddress.IPv4Address(0x7F000001))
        self.assertEqual(parse_peer_address("::1"), ipaddress.IPv6Address("::1"))
        self.assertEqual(parse_peer_address(ipaddress.IPv4Address("192.0.2.9")), ipaddress.IPv4Address("192.0.2.9"))
        for junk in (None, "", "  ", "some-host-name", "garbage", "300.1.1.1", "1.2.3", "192.0.2.7:80", "[::1]", 0, False):
            self.assertIsNone(parse_peer_address(junk), repr(junk))

    def test_is_loopback_peer(self):
        for address in ("127.0.0.2", "127.255.255.254", "::1", " ::1 ", "::ffff:127.0.0.2", "::ffff:7f00:1"):
            self.assertIs(is_loopback_peer(address), True, address)
        for address in ("192.0.2.7", "10.0.0.1", "128.0.0.1", "126.255.255.255", "::2", "2001:db8::1", "::ffff:10.0.0.1", "0.0.0.0", "::",
                        "some-host-name", "", None, "garbage", "127.0.0.2.5"):
            self.assertIs(is_loopback_peer(address), False, repr(address))


@tagged("post_install", "-at_install")
class TestTrustedClientIpUnits(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:get_trusted_client_ip]

    def test_a_forwarding_header_is_believed_only_from_a_loopback_peer(self):
        utils = _utils()
        headers = {"CF-Connecting-IP": "198.51.100.4", "X-Forwarded-For": "203.0.113.9, 10.0.0.1"}
        for remote in ("192.0.2.7", "10.0.0.1", "2001:db8::1", "128.0.0.1"):
            self.assertEqual(utils._get_trusted_client_ip(_request(remote, **headers)), remote, "the transport peer wins over a header")
        for remote in ("127.0.0.2", "::1", "::ffff:127.0.0.2"):
            self.assertEqual(utils._get_trusted_client_ip(_request(remote, **headers)), "198.51.100.4", remote)

    def test_the_cloudflare_header_wins_then_the_first_forwarded_for_entry_then_the_peer(self):
        utils = _utils()
        loop = "127.0.0.2"
        self.assertEqual(utils._get_trusted_client_ip(_request(loop, **{"CF-Connecting-IP": "  198.51.100.4  "})), "198.51.100.4")
        self.assertEqual(utils._get_trusted_client_ip(_request(loop, **{"CF-Connecting-IP": "198.51.100.4", "X-Forwarded-For": "1.1.1.1"})),
                         "198.51.100.4")
        self.assertEqual(utils._get_trusted_client_ip(_request(loop, **{"X-Forwarded-For": " 203.0.113.9 , 10.0.0.1"})), "203.0.113.9")
        self.assertEqual(utils._get_trusted_client_ip(_request(loop, **{"X-Forwarded-For": "203.0.113.9"})), "203.0.113.9")
        self.assertEqual(utils._get_trusted_client_ip(_request(loop, **{"CF-Connecting-IP": "", "X-Forwarded-For": "203.0.113.9"})),
                         "203.0.113.9", "an empty Cloudflare header falls through")
        self.assertEqual(utils._get_trusted_client_ip(_request(loop, **{"CF-Connecting-IP": "", "X-Forwarded-For": ""})), loop)
        self.assertEqual(utils._get_trusted_client_ip(_request(loop)), loop, "no header: the loopback peer itself")

    def test_a_peer_that_is_not_an_address_is_never_trusted(self):
        utils = _utils()
        for remote in (None, "", "garbage", "some-host-name"):
            self.assertEqual(utils._get_trusted_client_ip(_request(remote, **{"CF-Connecting-IP": "198.51.100.4"})), remote)

    def test_without_a_request_the_answer_is_none_and_the_current_request_is_used_by_default(self):
        utils = _utils()
        self.safe_patch_object(security_utils, "request", new=None)
        self.assertIsNone(utils._get_trusted_client_ip())
        current = _request("127.0.0.2", **{"CF-Connecting-IP": "198.51.100.4"})

        class Proxy:
            def __bool__(self):
                return True

            def _get_current_object(self):
                return current
        self.safe_patch_object(security_utils, "request", new=Proxy())
        self.assertEqual(utils._get_trusted_client_ip(), "198.51.100.4")
        self.assertEqual(utils._get_trusted_client_ip(_request("192.0.2.7")), "192.0.2.7", "a given request overrides the current one")
