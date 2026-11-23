# SPDX-License-Identifier: GPL-2.0-or-later
"""hams-node-update with the real apk-tools (SDK host binary) against a signed feed served over loopback HTTP, in a throwaway apk root."""

import functools
import http.server
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-update"
BUILD_FEED = HERE.parents[1] / "feed" / "build_feed.sh"
SDK_GLOB = "home/bruce/agent_work/*/sdk*/openwrt-sdk-*/staging_dir/host/bin/apk"


def find_apk():
    env = os.environ.get("HAMS_APK")
    if env:
        return env
    found = sorted(pathlib.Path("/").glob(SDK_GLOB)) or sorted(pathlib.Path("/").glob("home/bruce/agent_work/sdk_dl/x/openwrt-sdk-*/staging_dir/host/bin/apk"))
    return str(found[0]) if found else shutil.which("apk")


APK = find_apk()


def sh(*args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw)


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


@unittest.skipUnless(APK, "no apk-tools (set HAMS_APK or unpack the SDK)")
class UpdateTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        for d in ("cache", "apkroot", "keys", "cfg", "work", "bin", "fake"):
            (self.root / d).mkdir()
        self.key = self.root / "feed-private.pem"
        sh("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(self.key))
        self.pub = self.root / "feed-public.pem"
        self.pub.write_text(sh("openssl", "ec", "-in", str(self.key), "-pubout").stdout)
        self.repo = self.root / "repo.list"
        self.feed = self.root / "feed"
        # apk as the script calls it, pointed at the throwaway root, keys and repository list
        (self.root / "bin" / "apk").write_text('#!/bin/sh\nexec %s --root %s/apkroot --keys-dir %s/keys --repositories-file %s --cache-dir %s/cache "$@"\n'
                                               % (APK, self.root, self.root, self.repo, self.root))
        (self.root / "bin" / "uci").write_text('#!/bin/sh\ncase $1 in show) exit 0 ;; esac\nname=${3#hams_node_update.main.}\ncat "$FAKE/uci_$name" 2>/dev/null\nexit 0\n')
        for tool in ("apk", "uci"):
            (self.root / "bin" / tool).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.root / "fake"), PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]))
        (self.root / "cfg" / "hams_node").write_text("config gateway 'main'\n\toption prefix60 'good'\n")
        self.make_feed("1.0-r1")
        self.serve()
        self.setting("feed_url", self.url)
        self.setting("public_key", str(self.pub))
        self.setting("packages", "ht")

    def apk(self, *args):
        return sh(str(self.root / "bin" / "apk"), *args, env=self.env)

    def setting(self, name, value):
        (self.root / "fake" / ("uci_" + name)).write_text(value)

    def make_pkg(self, version):
        src = self.root / "src"
        shutil.rmtree(src, ignore_errors=True)
        (src / "usr/bin").mkdir(parents=True)
        (src / "usr/bin/ht").write_text("echo %s\n" % version)
        (src / "usr/bin/ht").chmod(0o755)
        out = self.root / "apks" / version
        out.mkdir(parents=True, exist_ok=True)
        done = sh(APK, "mkpkg", "--info", "name:ht", "--info", "version:" + version, "--info", "arch:noarch", "--info", "description:t",
                  "--info", "license:GPL-2.0-or-later", "--files", str(src), "--output", str(out / ("ht-%s.apk" % version)))
        self.assertEqual(done.returncode, 0, done.stderr)
        return out

    def make_feed(self, *versions, key=None):
        shutil.rmtree(self.feed, ignore_errors=True)
        dirs = [str(self.make_pkg(v)) for v in versions]
        done = subprocess.run(["sh", str(BUILD_FEED), "-k", str(key or self.key), "-o", str(self.feed), *dirs], capture_output=True, text=True, env=dict(os.environ, APK=APK))
        self.assertEqual(done.returncode, 0, done.stderr)

    def serve(self):
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=str(self.root)))  # burn-ignore-ssrf-test-value: loopback server of this test only
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        self.url = "http://127.0.0.1:%d/feed" % server.server_address[1]  # burn-ignore-ssrf-test-value: loopback server of this test only

    def install(self, version):
        """Install ht VERSION into the apk root straight from its file (the node's state before an update)."""
        self.repo.write_text("")
        pkg = self.root / "apks" / version / ("ht-%s.apk" % version)
        done = sh(APK, "--usermode", "--root", str(self.root / "apkroot"), "--keys-dir", str(self.root / "keys"), "--repositories-file", str(self.repo), "--cache-dir", str(self.root / "cache"),
                  "add", "--initdb", "--allow-untrusted", "--force-non-repository", str(pkg))
        self.assertEqual(done.returncode, 0, done.stderr)

    def installed(self):
        return self.apk("list", "-I", "ht").stdout.split(" ")[0]

    def update(self, *extra):
        return subprocess.run(["sh", str(SCRIPT), "-k", str(self.root / "keys"), "-r", str(self.repo), "-w", str(self.root / "work"), "-g", str(self.root / "cfg"),
                               "-c", str(self.root / "cron"), *extra, "run"], capture_output=True, text=True, env=self.env, timeout=120)

    def status(self):
        return (self.root / "work" / "status").read_text()

    def test_disabled_until_a_feed_url_and_key_are_configured(self):
        self.install("1.0-r1")
        self.make_feed("1.0-r1", "1.1-r1")
        for missing in ("feed_url", "public_key"):
            self.setting(missing, "")
            done = self.update()
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertEqual(self.installed(), "ht-1.0-r1")
            self.assertFalse(self.repo.read_text(), "nothing is written to the repository list while disabled")
            self.setting(missing, self.url if missing == "feed_url" else str(self.pub))

    def test_a_newer_signed_version_is_installed(self):
        self.install("1.0-r1")
        self.make_feed("1.0-r1", "1.1-r1")
        done = self.update()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(self.installed(), "ht-1.1-r1")
        self.assertIn("updated", self.status())
        self.assertFalse((self.root / "work" / "stage").exists(), "the staging directory is removed")
        again = self.update()
        self.assertEqual((again.returncode, self.installed()), (0, "ht-1.1-r1"))
        self.assertIn("up to date", self.status())

    def test_a_failed_health_check_rolls_back_the_package_and_the_settings(self):
        self.install("1.0-r1")
        self.make_feed("1.0-r1", "1.1-r1")
        self.setting("health_cmd", "echo broken > %s/cfg/hams_node; false" % self.root)
        done = self.update()
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertEqual(self.installed(), "ht-1.0-r1")
        self.assertIn("prefix60 'good'", (self.root / "cfg" / "hams_node").read_text())
        self.assertIn("rolled back", self.status())

    def test_an_index_signed_by_another_key_is_refused(self):
        self.install("1.0-r1")
        other = self.root / "other.pem"
        sh("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(other))
        self.make_feed("1.0-r1", "1.1-r1", key=other)
        done = self.update()
        self.assertEqual(done.returncode, 4, done.stderr)
        self.assertEqual(self.installed(), "ht-1.0-r1")

    def test_a_download_that_fails_changes_nothing(self):
        self.install("1.0-r1")
        self.make_feed("1.0-r1", "1.1-r1")
        (self.feed / "ht-1.1-r1.apk").unlink()
        done = self.update()
        self.assertEqual(done.returncode, 4, done.stderr)
        self.assertEqual(self.installed(), "ht-1.0-r1")
        self.assertFalse((self.root / "work" / "stage").exists())

    def test_no_update_is_started_when_the_previous_version_is_not_in_the_feed(self):
        self.install("1.0-r1")
        self.make_feed("1.1-r1")
        done = self.update()
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertEqual(self.installed(), "ht-1.0-r1")
        self.assertIn("rolled back to", self.status())

    def test_plain_http_feed_is_refused_unless_loopback(self):
        self.install("1.0-r1")
        self.setting("feed_url", "http://feed.example.test/hams")  # burn-ignore-ssrf-test-value: URL-scheme fixture only, nothing connects
        done = self.update()
        self.assertEqual(done.returncode, 1)
        self.assertIn("https://", done.stderr)

    def test_schedule_and_unschedule_edit_only_the_cron_line(self):
        (self.root / "cron").write_text("0 1 * * * other\n")
        for command, present in (("schedule", True), ("schedule", True), ("unschedule", False)):
            subprocess.run(["sh", str(SCRIPT), "-c", str(self.root / "cron"), command], check=True, env=self.env)
            text = (self.root / "cron").read_text()
            self.assertEqual("hams-node-update run" in text, present)
            self.assertEqual(text.count("hams-node-update run"), 1 if present else 0)
            self.assertIn("other", text)


if __name__ == "__main__":
    unittest.main()
