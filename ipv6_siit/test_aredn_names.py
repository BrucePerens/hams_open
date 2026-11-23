# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for aredn_names."""

import json
import unittest

import aredn_names
import siit_map

BLOCK = "2001:db8:ab01::/48"
EXPORT = {"block": BLOCK, "domains": [{"mesh": "bay", "domain": 7, "prefix96": None}]}
GATEWAY = {
    "mesh": "bay",
    "callsign": "K6BP",
    "hosts": {"W6XYZ-Hilltop.local.mesh": "10.1.2.3", "relay2": "10.1.2.4"},
    "services": [("Mail Web", "relay2"), ("lost", "nonode")],
}


class NameTests(unittest.TestCase):
    def test_ham_name_from_local_mesh_name(self):
        # Tests [@ANCHOR: aredn_names:ham_name]
        self.assertEqual(aredn_names.ham_name("W6XYZ-Hilltop.local.mesh", "bay", "K6BP"), "w6xyz-hilltop.bay.k6bp.ham")
        self.assertEqual(aredn_names.ham_name("relay2", "Bay", "k6bp/m"), "relay2.bay.k6bp-m.ham")

    def test_bad_labels_are_refused(self):
        # Tests [@ANCHOR: aredn_names:ham_name]
        # Tests [@ANCHOR: aredn_names:label]
        for host in ("", "-x", "a b", "a.b", "x" * 64):
            with self.assertRaises(ValueError):
                aredn_names.ham_name(host, "bay", "K6BP")

    def test_round_trip_to_local_mesh(self):
        # Tests [@ANCHOR: aredn_names:local_mesh_name]
        name = aredn_names.ham_name("relay2", "bay", "K6BP")
        self.assertEqual(aredn_names.local_mesh_name(name, "bay", "K6BP"), "relay2.local.mesh")
        with self.assertRaises(ValueError):
            aredn_names.local_mesh_name(name, "other", "K6BP")
        with self.assertRaises(ValueError):
            aredn_names.local_mesh_name("a.b.relay2.bay.k6bp.ham", "bay", "K6BP")


class ZoneTests(unittest.TestCase):
    def test_host_resolves_to_the_siit_address(self):
        # Tests [@ANCHOR: aredn_names:zone_text]
        text = aredn_names.zone_text(EXPORT, [GATEWAY])
        want = siit_map.embed(BLOCK, 7, "10.1.2.3")
        self.assertIn("w6xyz-hilltop.bay.k6bp 300 IN AAAA %s" % want, text.splitlines())

    def test_service_ad_becomes_a_name_and_unknown_host_is_skipped(self):
        # Tests [@ANCHOR: aredn_names:zone_text]
        lines = aredn_names.zone_text(EXPORT, [GATEWAY]).splitlines()
        self.assertIn("mail-web.bay.k6bp 300 IN CNAME relay2.bay.k6bp.ham.", lines)
        self.assertFalse([x for x in lines if x.startswith("lost.")])

    def test_inert_without_a_block(self):
        # Tests [@ANCHOR: aredn_names:zone_text]
        self.assertEqual(aredn_names.zone_text({"block": None, "domains": []}, [GATEWAY]), "")

    def test_withdrawn_or_unregistered_gateway_publishes_nothing(self):
        # Tests [@ANCHOR: aredn_names:zone_text]
        self.assertEqual(aredn_names.zone_text(EXPORT, []), "")
        self.assertEqual(aredn_names.zone_text(EXPORT, [dict(GATEWAY, mesh="gone")]), "")
        self.assertEqual(aredn_names.zone_text(json.dumps(EXPORT), [GATEWAY]), aredn_names.zone_text(EXPORT, [GATEWAY]))

    def test_same_ip_on_two_meshes_gets_different_addresses(self):
        # Tests [@ANCHOR: aredn_names:zone_text]
        export = {"block": BLOCK, "domains": [{"mesh": "bay", "domain": 7}, {"mesh": "hill", "domain": 8}]}
        other = dict(GATEWAY, mesh="hill")
        text = aredn_names.zone_text(export, [GATEWAY, other])
        self.assertEqual(len({x.split()[-1] for x in text.splitlines() if "w6xyz-hilltop" in x}), 2)


if __name__ == "__main__":
    unittest.main()
