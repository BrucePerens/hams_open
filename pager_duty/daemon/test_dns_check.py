# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The "dns" check type of generalized_monitor.py. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_dns_check.py
"""
# Tests [@ANCHOR: pager_duty:daemon_execute_check]
import os
import socket
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402

# `dig +trace` exits 0 and prints the name it was asked about (its own header line) for a name that does not exist.
NXDOMAIN_TRACE = (
    "; <<>> DiG 9.20.4 <<>> +trace gone.example.test\n"
    ";; global options: +cmd\n"
    ".\t\t86400\tIN\tNS\ta.root-servers.net.\n"
    ";; Received 1057 bytes from 199.7.91.13#53(d.root-servers.net) in 11 ms\n"
)
ANSWER_TRACE = NXDOMAIN_TRACE.replace("gone.example.test", "live.example.test") + "live.example.test.\t300\tIN\tA\t192.0.2.7\n"


class DnsCheckTests(unittest.TestCase):
    def check(self, stdout, resolver):
        done = mock.Mock(returncode=0, stdout=stdout, stderr="")
        domain = "live.example.test" if "live.example.test." in stdout else "gone.example.test"
        with mock.patch.object(gm, "ensure_executable", return_value=("/usr/bin/dig", "")), \
                mock.patch.object(gm.subprocess, "run", return_value=done), \
                mock.patch.object(gm.socket, "gethostbyname", side_effect=resolver) as lookup:
            return gm.execute_check({"type": "dns", "target": domain}), lookup

    def test_a_name_that_does_not_exist_fails_although_dig_exits_zero(self):
        (ok, msg), _lookup = self.check(NXDOMAIN_TRACE, socket.gaierror(-2, "Name or service not known"))
        self.assertFalse(ok, msg)
        self.assertIn("DNS resolution failed", msg)

    def test_a_traced_address_answer_passes(self):
        (ok, msg), lookup = self.check(ANSWER_TRACE, socket.gaierror(-2, "unused"))
        self.assertTrue(ok, msg)
        lookup.assert_not_called()

    def test_a_name_dig_shows_no_address_for_but_the_resolver_resolves_passes(self):
        (ok, msg), _lookup = self.check(NXDOMAIN_TRACE, lambda _name: "192.0.2.9")
        self.assertTrue(ok, msg)


if __name__ == "__main__":
    unittest.main()
