# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the generated Part 97 boundary ruleset: encrypted flows toward the mesh are refused and counted,
integrity-only TLS and plain flows pass and are station-ID logged."""

import re
import unittest

import ham_boundary


def chain(text, name):
    return re.search(r"chain %s \{(.*?)\n\t\}" % name, text, re.S).group(1)


def to_mesh(text):
    return [line.strip() for line in chain(text, "forward").splitlines() if 'oifname "br-dtdlink"' in line]


def first_match(text, proto, port):
    """The first forward rule toward the mesh that matches proto/port, as a (verdict, line) pair."""
    for line in to_mesh(text):
        if line.startswith('oifname "br-dtdlink" ') and not re.search(r"(tcp|udp) dport", line):
            continue
        m = re.search(r"%s dport \{ ([\d, ]+) \}" % proto, line)
        if m and str(port) in [p.strip() for p in m.group(1).split(",")]:
            return line.split()[-1], line
    return None, None


class BoundaryTests(unittest.TestCase):
    def setUp(self):
        self.text = ham_boundary.render_ruleset({"wg_port": 51821})

    def test_wireguard_toward_the_mesh_is_dropped_and_counted(self):
        # Tests [@ANCHOR: ham_boundary:render_ruleset]
        for port in (51821, 51820, 5525):
            verdict, line = first_match(self.text, "udp", port)
            self.assertEqual(verdict, "drop", port)
            self.assertIn("refused_encrypted", line)

    def test_encrypting_tls_and_other_encrypted_ports_are_dropped_and_counted(self):
        for port in (443, 853, 22, 993):
            verdict, line = first_match(self.text, "tcp", port)
            self.assertEqual(verdict, "drop", port)
            self.assertIn("refused_encrypted", line)

    def test_esp_is_dropped(self):
        self.assertTrue(any("esp" in r and "drop" in r for r in to_mesh(self.text)))

    def test_integrity_only_tls_port_and_plain_http_pass_with_station_id_log(self):
        for port in (8443, 80):
            verdict, line = first_match(self.text, "tcp", port)
            self.assertEqual(verdict, "accept", port)
            self.assertIn("log group 97 prefix", line)
            self.assertIn("ct state new", line)

    def test_everything_else_toward_the_mesh_is_dropped_by_default(self):
        self.assertTrue(to_mesh(self.text)[-1].endswith("counter name refused_other drop"))

    def test_encrypted_rules_come_before_any_accept_for_a_port(self):
        rules = to_mesh(self.text)
        first_accept = min(i for i, r in enumerate(rules) if r.endswith("accept"))
        self.assertTrue(all("refused_encrypted" in r for r in rules[:first_accept]))
        self.assertGreaterEqual(first_accept, 3)

    def test_the_wireguard_overlay_port_is_refused_from_the_mesh_side(self):
        self.assertIn('iifname "br-dtdlink" udp dport 51821 drop', chain(self.text, "input"))

    def test_counters_are_declared(self):
        for name in ("refused_encrypted", "refused_other", "forwarded_to_mesh"):
            self.assertIn("counter %s { }" % name, self.text)

    def test_allowing_an_encrypted_port_toward_the_mesh_is_refused(self):
        with self.assertRaises(ValueError):
            ham_boundary.render_ruleset({"plain_tcp_ports": [443]})
        with self.assertRaises(ValueError):
            ham_boundary.render_ruleset({"ham_tls_port": 22})
        with self.assertRaises(ValueError):
            ham_boundary.render_ruleset({"plain_udp_ports": [51820]})

    def test_other_interfaces_are_not_filtered(self):
        self.assertIn('oifname != "br-dtdlink" accept', chain(self.text, "forward"))

    def test_mesh_interface_comes_from_config(self):
        text = ham_boundary.render_ruleset({"mesh_if": "wlan0-mesh"})
        self.assertIn('oifname "wlan0-mesh" counter name refused_other drop', text)


if __name__ == "__main__":
    unittest.main()
