# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for siit_map."""

import json
import unittest

import siit_map

BLOCK = "2001:db8:ab01::/48"


class MappingTests(unittest.TestCase):
    def test_embed_matches_the_proposal_example(self):
        # Tests [@ANCHOR: siit_map:embed]
        self.assertEqual(str(siit_map.embed(BLOCK, 7, "10.1.2.3")), "2001:db8:ab01:7::a01:203")

    def test_extreme_domain_ids(self):
        # Tests [@ANCHOR: siit_map:embed]
        # Tests [@ANCHOR: siit_map:extract]
        for domain in (0, 65535):
            address = siit_map.embed(BLOCK, domain, "10.255.0.1")
            self.assertEqual(siit_map.extract(BLOCK, address), (domain, "10.255.0.1"))

    def test_overlapping_meshes_do_not_collide(self):
        # Tests [@ANCHOR: siit_map:embed]
        self.assertNotEqual(siit_map.embed(BLOCK, 1, "10.1.2.3"), siit_map.embed(BLOCK, 2, "10.1.2.3"))

    def test_extract_rejects_other_blocks_and_nonzero_middle(self):
        # Tests [@ANCHOR: siit_map:extract]
        with self.assertRaises(ValueError):
            siit_map.extract(BLOCK, "2001:db8:ab02:7::a01:203")
        with self.assertRaises(ValueError):
            siit_map.extract(BLOCK, "2001:db8:ab01:7:0:1:a01:203")

    def test_well_known_nat64_prefix_is_refused(self):
        # Tests [@ANCHOR: siit_map:embed]
        # Tests [@ANCHOR: siit_map:domain_prefix]
        with self.assertRaises(ValueError):
            siit_map.embed("64:ff9b::/48", 1, "10.0.0.1")
        with self.assertRaises(ValueError):
            siit_map.domain_prefix("64:ff9b::/48", 1)

    def test_bad_input(self):
        # Tests [@ANCHOR: siit_map:embed]
        # Tests [@ANCHOR: siit_map:block]
        # Tests [@ANCHOR: siit_map:domain]
        with self.assertRaises(ValueError):
            siit_map.embed(BLOCK, 65536, "10.0.0.1")
        with self.assertRaises(ValueError):
            siit_map.embed("2001:db8::/32", 1, "10.0.0.1")
        with self.assertRaises(ValueError):
            siit_map.embed(BLOCK, 1, "::1")

    def test_domain_prefix(self):
        # Tests [@ANCHOR: siit_map:domain_prefix]
        self.assertEqual(str(siit_map.domain_prefix(BLOCK, 7)), "2001:db8:ab01:7::/96")

    def test_next_free_domain(self):
        # Tests [@ANCHOR: siit_map:next_free_domain]
        self.assertEqual(siit_map.next_free_domain([]), 1)
        self.assertEqual(siit_map.next_free_domain([1, 2, 4]), 3)
        with self.assertRaises(ValueError):
            siit_map.next_free_domain(range(1, 65536))

    def test_jool_config(self):
        # Tests [@ANCHOR: siit_map:jool_siit_config]
        config = json.loads(siit_map.jool_siit_config(BLOCK, 7, "hams-mesh7"))
        self.assertEqual(config["instance"], "hams-mesh7")
        self.assertEqual(config["framework"], "netfilter")
        self.assertEqual(config["global"]["pool6"], "2001:db8:ab01:7::/96")
        with self.assertRaises(ValueError):
            siit_map.jool_siit_config("64:ff9b::/48", 7, "x")


if __name__ == "__main__":
    unittest.main()


class GatewayConfigTests(unittest.TestCase):
    # Fixture in the exact format of ham_ipv6net ham.ipv6.siit.domain._gateway_config() (hams_com #10625).
    EXPORT = (
        '{"block": "2001:db8:ab01::/48", "domains": ['
        '{"domain": 1, "mesh": "mesh-a", "prefix96": "2001:db8:ab01:1::/96"}, '
        '{"domain": 2, "mesh": "mesh-b", "prefix96": null}]}'
    )

    def test_one_config_per_domain_from_the_export(self):
        # Tests [@ANCHOR: siit_map:gateway_configs]
        out = siit_map.gateway_configs(self.EXPORT)
        self.assertEqual(sorted(out), ["mesh-a", "mesh-b"])
        self.assertEqual(json.loads(out["mesh-a"])["global"]["pool6"], "2001:db8:ab01:1::/96")
        self.assertEqual(json.loads(out["mesh-b"])["global"]["pool6"], "2001:db8:ab01:2::/96")

    def test_unprovisioned_registry_is_empty_and_bad_exports_are_rejected(self):
        # Tests [@ANCHOR: siit_map:gateway_configs]
        self.assertEqual(siit_map.gateway_configs('{"block": null, "domains": []}'), {})
        bad = [
            '{"block": null, "domains": [{"domain": 1, "mesh": "m", "prefix96": null}]}',
            '{"block": "2001:db8:ab01::/48", "domains": [{"domain": 1, "mesh": "m", "prefix96": "2001:db8:ab01:9::/96"}]}',
            '{"block": "2001:db8:ab01::/48", "domains": [{"domain": 1, "mesh": "a"}, {"domain": 1, "mesh": "b"}]}',
            '{"block": "2001:db8:ab01::/48", "domains": [{"domain": 1, "mesh": "a"}, {"domain": 2, "mesh": "a"}]}',
            '{"block": "64:ff9b::/48", "domains": [{"domain": 1, "mesh": "a"}]}',
            '{"block": "2001:db8:ab01::/48"}',
        ]
        for text in bad:
            with self.assertRaises(ValueError, msg=text):
                siit_map.gateway_configs(text)
