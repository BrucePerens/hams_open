# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The domain DNS pre-check of the "certbot" check type. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_certbot_domain_dns.py
"""
# Tests [@ANCHOR: pager_duty:daemon_execute_check]
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402

MY_IP = "198.51.100.1"


def reply(status, *answers):
    return f";; ->>HEADER<<- opcode: QUERY, status: {status}, id: 1\n" + "".join(a + "\n" for a in answers)


def dig_for(table):
    """table maps (domain, type) to dig output; a missing key means dig printed nothing (no server answered)."""
    def run(cmd, **_kw):
        return mock.Mock(returncode=0, stdout=table.get((cmd[-2], cmd[-1]), ""), stderr="")
    return run


class CertbotDomainTests(unittest.TestCase):
    def problem(self, table, domain="w7com.hams.com"):
        with mock.patch.object(gm, "ensure_executable", return_value=("/usr/bin/dig", "")), \
                mock.patch.object(gm.subprocess, "run", side_effect=dig_for(table)):
            return gm.certbot_domain_problem(domain, MY_IP)

    def test_nxdomain(self):
        msg = self.problem({("x.test", "A"): reply("NXDOMAIN"), ("x.test", "AAAA"): reply("NXDOMAIN")}, "x.test")
        self.assertIn("does not exist", msg)
        self.assertIn("NXDOMAIN", msg)

    def test_name_without_address_records(self):
        msg = self.problem({("w7com.hams.com", "A"): reply("NOERROR"), ("w7com.hams.com", "AAAA"): reply("NOERROR")})
        self.assertIn("w7com.hams.com exists but has no address (A or AAAA) record", msg)
        self.assertIn(MY_IP, msg)

    def test_wrong_address_names_both_ips(self):
        d = "w7com.hams.com"
        msg = self.problem({(d, "A"): reply("NOERROR", f"{d}. 300 IN A 192.0.2.9"), (d, "AAAA"): reply("NOERROR")})
        self.assertIn("192.0.2.9", msg)
        self.assertIn(MY_IP, msg)

    def test_correct_address_passes(self):
        d = "w7com.hams.com"
        table = {(d, "A"): reply("NOERROR", f"{d}. 300 IN A {MY_IP}"), (d, "AAAA"): reply("NOERROR")}
        self.assertIsNone(self.problem(table))

    def test_resolver_failure_is_not_called_missing_record(self):
        d = "w7com.hams.com"
        msg = self.problem({(d, "A"): reply("SERVFAIL"), (d, "AAAA"): reply("SERVFAIL")})
        self.assertIn("SERVFAIL", msg)
        self.assertNotIn("no address", msg)

    def test_timeout(self):
        msg = self.problem({})
        self.assertIn("timed out", msg)

    def test_every_failing_domain_is_reported_capped(self):
        names = [f"d{i}.test" for i in range(13)]
        table = {}
        for n in names:
            table[(n, "A")] = reply("NXDOMAIN")
            table[(n, "AAAA")] = reply("NXDOMAIN")
        ok_name = "ok.test"
        table[(ok_name, "A")] = reply("NOERROR", f"{ok_name}. 1 IN A {MY_IP}")
        table[(ok_name, "AAAA")] = reply("NOERROR")
        urlopen = mock.MagicMock()
        urlopen.return_value.__enter__.return_value.status = 200
        urlopen.return_value.__enter__.return_value.read.return_value = MY_IP.encode()
        with mock.patch.object(gm, "ensure_executable", return_value=("/usr/bin/dig", "")), \
                mock.patch.object(gm.subprocess, "run", side_effect=dig_for(table)), \
                mock.patch.object(gm.urllib.request, "urlopen", urlopen):
            ok, msg = gm.execute_check({"type": "certbot", "target": ",".join([ok_name] + names)})
        self.assertFalse(ok)
        lines = msg.split("\n")
        self.assertEqual(len(lines), 10)
        self.assertIn("d0.test", lines[0])
        self.assertTrue(lines[-1].endswith("(and 3 more)"), lines[-1])
        self.assertNotIn(ok_name, msg)


if __name__ == "__main__":
    unittest.main()
