# SPDX-License-Identifier: GPL-2.0-or-later
"""The http:// exception for 127.0.0.1 and localhost must not be satisfiable by userinfo that moves the real host elsewhere."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SBIN = HERE.parent / "files" / "usr" / "sbin"
EVIL = ("http://127.0.0.1:80@evil.example/", "http://localhost:x@evil.example", "http://127.0.0.1:pw@evil.example:8080/x")  # burn-ignore-ssrf-test-value: URL fixtures only, nothing connects
SCRIPTS = {"hams-node-enroll": ["-u", "{url}", "-s", "{state}", "fetch"], "hams-node-key": ["-u", "{url}", "-s", "{state}", "rotate"],
           "hams-node-endpoint": ["-u", "{url}", "-s", "{state}", "update"]}


class LoopbackUrlTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        bindir = self.root / "bin"
        bindir.mkdir()
        (bindir / "uclient-fetch").write_text('#!/bin/sh\necho "$@" >> "%s/fetched"\nexit 1\n' % self.root)
        (bindir / "uci").write_text('#!/bin/sh\ncase $* in *hams_node_update.main.feed_url*) echo "$FEED" ;; *hams_node_update.main.public_key*) echo /dev/null ;; esac\nexit 0\n')
        (bindir / "apk").write_text('#!/bin/sh\necho "$@" >> "%s/fetched"\nexit 1\n' % self.root)
        for f in bindir.iterdir():
            f.chmod(0o755)
        self.env = dict(os.environ, PATH="%s:%s" % (bindir, os.environ["PATH"]), FEED="")

    def test_userinfo_cannot_turn_the_loopback_exception_into_another_host(self):
        for name, args in SCRIPTS.items():
            for url in EVIL:
                with self.subTest(script=name, url=url):
                    cmd = ["sh", str(SBIN / name)] + [a.format(url=url, state=self.root / "state") for a in args]
                    result = subprocess.run(cmd, env=self.env, capture_output=True, text=True, timeout=30)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn("https://", result.stderr)
        self.assertFalse((self.root / "fetched").exists())

    def test_update_feed_url_with_userinfo_is_refused(self):
        for url in EVIL:
            with self.subTest(url=url):
                env = dict(self.env, FEED=url)
                work = self.root / "work"
                cmd = ["sh", str(SBIN / "hams-node-update"), "-k", str(self.root / "k"), "-r", str(self.root / "r"), "-w", str(work), "run"]
                result = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertFalse((self.root / "fetched").exists())

    def test_real_loopback_urls_still_pass_the_check(self):
        cmd = ["sh", str(SBIN / "hams-node-key"), "-u", "http://127.0.0.1:8097", "-s", str(self.root / "state"), "rotate"]  # burn-ignore-ssrf-test-value: loopback fixture
        result = subprocess.run(cmd, env=self.env, capture_output=True, text=True, timeout=30)
        self.assertNotIn("portal URL must be https", result.stderr)


if __name__ == "__main__":
    unittest.main()
