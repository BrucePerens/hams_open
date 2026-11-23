# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for ham_policy: whitelist of permitted sites and the station-identification log."""

import io
import unittest

import ham_policy


class WhitelistTests(unittest.TestCase):
    def setUp(self):
        self.whitelist = ham_policy.Whitelist(["example.org", "*.wiki.test"])

    def test_ham_names_are_always_permitted(self):
        # Tests [@ANCHOR: ham_policy:whitelist]
        # Tests [@ANCHOR: ham_policy:whitelist_init]
        self.assertTrue(self.whitelist.permits("node.ham"))
        self.assertTrue(self.whitelist.permits("A.B.HAM."))

    def test_listed_site_and_wildcard_subdomains_are_permitted(self):
        self.assertTrue(self.whitelist.permits("Example.org"))
        self.assertTrue(self.whitelist.permits("en.wiki.test"))

    def test_unlisted_sites_are_refused(self):
        for host in ("evil.com", "notexample.org", "example.org.evil.com", "wiki.test", "hamx", ""):
            self.assertFalse(self.whitelist.permits(host), host)


class StationIdLogTests(unittest.TestCase):
    def test_log_line_names_station_destination_suite_and_time(self):
        # Tests [@ANCHOR: ham_policy:log_connection]
        out = io.StringIO()
        ham_policy.log_connection(out, "w1aw", "node.ham", "NULL-SHA256", now=1700000000)
        line = out.getvalue()
        for part in ("2023-11-14T22:13:20Z", "W1AW", "node.ham", "NULL-SHA256"):
            self.assertIn(part, line)
        self.assertTrue(line.endswith("\n"))

    def test_a_connection_without_a_callsign_is_not_logged(self):
        with self.assertRaises(ValueError):
            ham_policy.log_connection(io.StringIO(), "", "node.ham", "NULL-SHA256")

    def test_a_log_time_out_of_range_is_a_value_error(self):
        for bad in (1e30, -1e30, 2**80, float("nan")):
            with self.assertRaises(ValueError, msg=repr(bad)):
                ham_policy.log_connection(io.StringIO(), "w1aw", "node.ham", "NULL-SHA256", now=bad)


if __name__ == "__main__":
    unittest.main()
