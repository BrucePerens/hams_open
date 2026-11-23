# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-dns with a stand-in for uci (reads KEY=VALUE lines)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-dns"
UCI = '#!/bin/sh\nkey=${3#hams_node_dns.main.}\ngrep "^$key=" "$FAKE/uci" | head -n 1 | cut -d= -f2-\n'
BASE = {"enabled": "1", "prefix": "2001:db8:ab00::/40", "resolver": "2001:db8:ab00::53 2001:db8:ab00::54"}


class DnsTests(unittest.TestCase):
    def render(self, **over):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        (root / "bin").mkdir()
        (root / "bin" / "uci").write_text(UCI)
        (root / "bin" / "uci").chmod(0o755)
        (root / "uci").write_text("".join("%s=%s\n" % kv for kv in dict(BASE, **over).items()))
        out = root / "d" / "hams.conf"
        env = dict(os.environ, PATH="%s:%s" % (root / "bin", os.environ["PATH"]), FAKE=str(root))
        result = subprocess.run(["sh", str(SCRIPT), "render", str(out)], env=env, capture_output=True, text=True)
        return result, (out.read_text().splitlines() if out.exists() else None)

    def test_ham_and_reverse_zone_forwarded(self):
        result, lines = self.render()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("server=/ham/2001:db8:ab00::53", lines)
        self.assertIn("server=/ham/2001:db8:ab00::54", lines)
        self.assertIn("server=/b.a.8.b.d.0.1.0.0.2.ip6.arpa/2001:db8:ab00::53", lines)

    def test_public_names_do_not_go_to_ham_resolvers(self):
        _, lines = self.render()
        for line in lines:
            if line.startswith("#"):
                continue
            self.assertRegex(line, r"^server=/[^/]+/", "every server line is limited to a zone: " + line)
            self.assertNotRegex(line, r"^server=/(\.|/)")

    def test_no_prefix_forwards_ham_only(self):
        _, lines = self.render(prefix="")
        self.assertEqual([l for l in lines if "ip6.arpa" in l], [])
        self.assertEqual(len([l for l in lines if l.startswith("server=/ham/")]), 2)

    def test_disabled_or_no_resolvers_writes_nothing(self):
        for over in ({"enabled": "0"}, {"resolver": ""}):
            result, lines = self.render(**over)
            self.assertEqual(result.returncode, 0)
            self.assertIsNone(lines)

    def test_bad_input_refused(self):
        for over in ({"resolver": "1.2.3.4;rm"}, {"prefix": "2001:db8::/41"}, {"prefix": "zz::/40"}):
            result, lines = self.render(**over)
            self.assertNotEqual(result.returncode, 0, over)
            self.assertIsNone(lines)


if __name__ == "__main__":
    unittest.main()
