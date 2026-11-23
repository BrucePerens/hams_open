# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for aredn_resolver."""

import unittest

import aredn_resolver

RESOLVERS = ["2001:db8:ab00::53", "2001:db8:ab00::54"]


class ResolverTests(unittest.TestCase):
    def test_forwards_ham_to_each_resolver(self):
        # Tests [@ANCHOR: aredn_resolver:dnsmasq_conf]
        lines = aredn_resolver.dnsmasq_conf(RESOLVERS).splitlines()
        self.assertIn("server=/ham/2001:db8:ab00::53", lines)
        self.assertIn("server=/ham/2001:db8:ab00::54", lines)

    def test_local_mesh_is_left_to_aredn(self):
        # Tests [@ANCHOR: aredn_resolver:dnsmasq_conf]
        text = aredn_resolver.dnsmasq_conf(RESOLVERS)
        self.assertNotIn("local.mesh", text)
        self.assertEqual([x for x in text.splitlines() if x and not x.startswith("#")][0][:11], "server=/ham")
        self.assertFalse([x for x in text.splitlines() if x.startswith("address=") or x.startswith("local=")])

    def test_inert_without_resolvers(self):
        # Tests [@ANCHOR: aredn_resolver:dnsmasq_conf]
        self.assertEqual(aredn_resolver.dnsmasq_conf([]), "")

    def test_bad_resolvers_are_refused(self):
        # Tests [@ANCHOR: aredn_resolver:dnsmasq_conf]
        for bad in ("10.0.0.1", "not-an-address", "/ham/::1", "::1\nserver=/local.mesh/1.1.1.1"):
            with self.assertRaises(ValueError):
                aredn_resolver.dnsmasq_conf([bad])


if __name__ == "__main__":
    unittest.main()
