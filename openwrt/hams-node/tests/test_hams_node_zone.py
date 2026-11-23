# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-zone with a stand-in for uci (reads KEY=VALUE lines)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-zone"
UCI = '#!/bin/sh\nkey=${3#hams_node_zone.main.}\ngrep "^$key=" "$FAKE/uci" | head -n 1 | cut -d= -f2-\n'
BASE = {"enabled": "1", "primary": "2001:db8:ab00::53 2001:db8:ab00::54", "port": "5354"}


class ZoneTests(unittest.TestCase):
    def render(self, **over):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        (root / "bin").mkdir()
        (root / "bin" / "uci").write_text(UCI)
        (root / "bin" / "uci").chmod(0o755)
        cfg = dict(BASE, zonefile=str(root / "ham.zone"), **over)
        (root / "uci").write_text("".join("%s=%s\n" % kv for kv in cfg.items()))
        env = dict(os.environ, PATH="%s:%s" % (root / "bin", os.environ["PATH"]), FAKE=str(root))
        result = subprocess.run(["sh", str(SCRIPT), "render", str(root / "unbound.conf")],
                                env=env, capture_output=True, text=True)
        files = [(root / n).read_text() if (root / n).exists() else None for n in ("unbound.conf",)]
        return result, files[0], None, root

    def test_zone_is_cached_on_the_node_and_served_locally_with_anycast_as_fallback(self):
        result, unbound, dnsmasq, root = self.render()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('name: "ham."', unbound)
        self.assertIn("primary: 2001:db8:ab00::53", unbound)
        self.assertIn("primary: 2001:db8:ab00::54", unbound)
        self.assertIn('zonefile: "%s"' % (root / "ham.zone"), unbound)
        self.assertIn("for-downstream: yes", unbound)
        self.assertIn("fallback-enabled: yes", unbound)
        self.assertIn("interface: 127.0.0.1@5354", unbound)  # burn-ignore-ssrf-test-value: text compared with a rendered unbound config, nothing connects

    def test_trust_anchor_when_configured(self):
        _, unbound, _, _ = self.render(trust_anchor="ham. IN DS 12345 13 2 ABCDEF0123")
        self.assertIn('trust-anchor: "ham. IN DS 12345 13 2 ABCDEF0123"', unbound)

    def test_disabled_or_no_primary_writes_nothing(self):
        for over in ({"enabled": "0"}, {"primary": ""}):
            result, unbound, dnsmasq, _ = self.render(**over)
            self.assertEqual(result.returncode, 0)
            self.assertIsNone(unbound)
            self.assertIsNone(dnsmasq)

    def test_bad_input_refused(self):
        for over in ({"primary": "2001:db8::1;x"}, {"port": "53; evil"}, {"trust_anchor": 'x"\nserver:'}):
            result, unbound, _, _ = self.render(**over)
            self.assertNotEqual(result.returncode, 0, over)
            self.assertIsNone(unbound)


if __name__ == "__main__":
    unittest.main()
