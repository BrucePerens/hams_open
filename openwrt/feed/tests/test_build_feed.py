# SPDX-License-Identifier: GPL-2.0-or-later
"""build_feed.sh makes a signed apk index that apk verifies with the public key only."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "build_feed.sh"
SDK_GLOB = "/home/bruce/agent_work/sdk_dl/x/openwrt-sdk-*/staging_dir/host/bin/apk"


def find_apk():
    env = os.environ.get("HAMS_APK")
    if env:
        return env
    found = sorted(pathlib.Path("/").glob(SDK_GLOB.lstrip("/")))
    return str(found[0]) if found else shutil.which("apk")


APK = find_apk()
PKG = pathlib.Path(os.environ.get("HAMS_TEST_APK", ""))


def sh(*args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw)


@unittest.skipUnless(APK, "no apk-tools (set HAMS_APK or unpack the SDK)")
class BuildFeedTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.key = self.root / "test-private.pem"
        self.pub = self.root / "test-public.pem"
        sh("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(self.key))
        pub = sh("openssl", "ec", "-in", str(self.key), "-pubout")
        self.pub.write_text(pub.stdout)
        # a tiny package built with apk mkpkg
        src = self.root / "src"
        (src / "usr/bin").mkdir(parents=True)
        (src / "usr/bin/hams-test").write_text("#!/bin/sh\necho hi\n")
        (src / "usr/bin/hams-test").chmod(0o755)
        self.apks = self.root / "apks"
        self.apks.mkdir()
        mk = sh(
            APK, "mkpkg", "--info", "name:hams-test", "--info", "version:1.0-r1", "--info", "arch:noarch",
            "--info", "description:test", "--info", "license:GPL-2.0-or-later",
            "--files", str(src), "--output", str(self.apks / "hams-test-1.0-r1.apk"),
        )
        self.assertEqual(mk.returncode, 0, mk.stderr)

    def build(self, *extra, key=None):
        out = self.root / "feed"
        env = dict(os.environ, APK=APK)
        return out, subprocess.run(
            ["sh", str(SCRIPT), "-k", str(key or self.key), "-o", str(out), *extra, str(self.apks)],
            capture_output=True, text=True, env=env,
        )

    def test_index_is_signed_and_verifies_with_public_key(self):
        out, res = self.build()
        self.assertEqual(res.returncode, 0, res.stderr)
        (self.root / "r").mkdir()
        keys = self.root / "keys"
        keys.mkdir()
        shutil.copy(self.pub, keys / "hams.pem")
        self.assertTrue((out / "packages.adb").is_file())
        self.assertTrue((out / "hams-test-1.0-r1.apk").is_file())
        ok = sh(APK, "--root", str(self.root / "r"), "--keys-dir", str(keys), "--repository", str(out / "packages.adb"),
                "--no-cache", "search", "hams-test")
        self.assertEqual(ok.returncode, 0, ok.stderr)
        self.assertIn("hams-test", ok.stdout)
        self.assertNotIn("UNTRUSTED", ok.stderr)

    def test_index_is_untrusted_with_a_different_key(self):
        out, res = self.build()
        self.assertEqual(res.returncode, 0, res.stderr)
        other = self.root / "other.pem"
        sh("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(other))
        (self.root / "r2").mkdir()
        keys = self.root / "wrongkeys"
        keys.mkdir()
        (keys / "other.pem").write_text(sh("openssl", "ec", "-in", str(other), "-pubout").stdout)
        bad = sh(APK, "--root", str(self.root / "r2"), "--keys-dir", str(keys), "--repository", str(out / "packages.adb"),
                 "--no-cache", "search", "hams-test")
        self.assertIn("UNTRUSTED", bad.stderr)

    def test_refuses_a_key_inside_the_repository(self):
        repo = pathlib.Path(__file__).resolve().parents[3]
        out, res = self.build(key=repo / "openwrt" / "feed" / "private-key.pem")
        self.assertNotEqual(res.returncode, 0)

    def test_publishes_the_public_key_next_to_the_index(self):
        out, res = self.build()
        self.assertEqual(res.returncode, 0, res.stderr)
        self.assertEqual((out / "hams-feed-public.pem").read_text(), self.pub.read_text())


if __name__ == "__main__":
    unittest.main()
