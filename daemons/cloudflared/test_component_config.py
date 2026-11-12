# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests of the config classes of component-tests/config.py (pure data; no tunnel or network needed)."""
import base64
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "component-tests"))
import config as cfg  # noqa: E402


class ConfigClassesTest(unittest.TestCase):
    # Tests [@ANCHOR: cloudflared:BaseConfig]
    def test_base_config_adds_no_autoupdate_and_metrics_and_is_frozen(self):
        base = cfg.BaseConfig(cloudflared_binary="cloudflared")
        merged = base.merge_config({"x": 1})
        self.assertEqual(merged["x"], 1)
        self.assertTrue(merged["no-autoupdate"])
        self.assertIn("localhost:", merged["metrics"])
        with self.assertRaises(Exception):
            base.no_autoupdate = False

    # Tests [@ANCHOR: cloudflared:NamedTunnelBaseConfig]
    def test_named_base_config_requires_tunnel_credentials_and_ingress(self):
        with self.assertRaises(TypeError):
            cfg.NamedTunnelBaseConfig(cloudflared_binary="c", credentials_file="f", ingress=[])
        with self.assertRaises(TypeError):
            cfg.NamedTunnelBaseConfig(cloudflared_binary="c", tunnel="t", ingress=[])
        with self.assertRaises(TypeError):
            cfg.NamedTunnelBaseConfig(cloudflared_binary="c", tunnel="t", credentials_file="f")

    # Tests [@ANCHOR: cloudflared:NamedTunnelConfig]
    def test_named_config_merges_defaults_gives_url_token_and_a_base_config_without_the_tunnel(self):
        with tempfile.TemporaryDirectory() as tmp:
            creds = os.path.join(tmp, "creds.json")
            with open(creds, "w") as handle:
                json.dump({"AccountTag": "acct", "TunnelID": "tid", "TunnelSecret": "sec"}, handle)
            named = cfg.NamedTunnelConfig(cloudflared_binary="c", tunnel="tid", credentials_file=creds,
                                          ingress=[{"service": "http_status:404"}], hostname="h.example.com",
                                          additional_config={"ingress": [{"service": "hello_world"}]})
            self.assertEqual(named.full_config["ingress"], [{"service": "hello_world"}])
            self.assertEqual(named.get_url(), "https://h.example.com")
            self.assertEqual(named.get_tunnel_id(), "tid")
            self.assertEqual(json.loads(base64.b64decode(named.get_token())), {"a": "acct", "t": "tid", "s": "sec"})
            base = named.base_config()
            self.assertNotIn("tunnel", base)
            self.assertNotIn("credentials-file", base)
            self.assertIn("tunnel", named.full_config)

    # Tests [@ANCHOR: cloudflared:QuickTunnelConfig]
    def test_quick_config_is_base_config_plus_the_additional_keys(self):
        quick = cfg.QuickTunnelConfig(cloudflared_binary="c", additional_config={"url": "http://localhost:80"})
        self.assertEqual(quick.full_config["url"], "http://localhost:80")
        self.assertTrue(quick.full_config["no-autoupdate"])


if __name__ == "__main__":
    unittest.main()
