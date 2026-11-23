# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-report with stand-ins for uclient-fetch (one canned reply, request logged) and the real jsonfilter stand-in."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-report"
FETCH = """#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
post = [a for a in args if a.startswith("--post-data=")][0]
open(os.environ["FAKE"] + "/calls", "a").write(args[-1] + " " + post + "\\n")
reply = os.environ["FAKE"] + "/reply"
if not os.path.exists(reply):
    sys.exit(1)
open(args[args.index("-O") + 1], "w").write(open(reply).read())
"""
HOSTS = "10.1.2.3\tn0call-hap\n10.1.2.4\tn0call-tower\nnot-an-ip\tbad\n10.1.2.5\tbad;x\n"
SERVICES = "http://n0call-hap.local.mesh:80/|tcp|Web Cam\nhttp://n0call-tower:8080/x|tcp|Chat \"x\"\nbroken\n"


class ReportTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.fake = self.root / "fake"
        self.fake.mkdir()
        self.state = self.root / "state"
        self.state.mkdir()
        (self.state / "device_token").write_text("tok_en-1\n")
        self.cron = self.root / "crontab"
        (self.root / "hosts").write_text(HOSTS)
        (self.root / "services").write_text(SERVICES)
        bindir = self.root / "bin"
        bindir.mkdir()
        (bindir / "uclient-fetch").write_text(FETCH)
        (bindir / "uclient-fetch").chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s:%s" % (bindir, HERE, os.environ["PATH"]))

    def run_script(self, command="run"):
        return subprocess.run(["sh", str(SCRIPT), "-u", "https://portal.example", "-s", str(self.state), "-c", str(self.cron),
                               "-H", str(self.root / "hosts"), "-S", str(self.root / "services"), command],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def reply(self, status):
        (self.fake / "reply").write_text(json.dumps({"jsonrpc": "2.0", "id": None, "result": {"status": status}}))

    def sent(self):
        url, post = (self.fake / "calls").read_text().splitlines()[0].split(" ", 1)
        return url, json.loads(post[len("--post-data="):])["params"]

    def test_posts_hosts_and_services_with_the_token(self):
        self.reply("success")
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        url, params = self.sent()
        self.assertEqual(url, "https://portal.example/api/ipv6/gateway/report")
        self.assertEqual(params["token"], "tok_en-1")
        self.assertEqual(params["hosts"], {"n0call-hap": "10.1.2.3", "n0call-tower": "10.1.2.4"})
        self.assertEqual(params["services"], [["Web Cam", "n0call-hap.local.mesh"], ["Chat x", "n0call-tower"]])

    def test_refusal_exits_1_and_unreachable_exits_4(self):
        self.reply("error")
        self.assertEqual(self.run_script().returncode, 1)
        (self.fake / "reply").unlink()
        self.assertEqual(self.run_script().returncode, 4)

    def test_not_enrolled_or_revoked_sends_nothing(self):
        (self.state / "status").write_text("revoked\n")
        self.assertEqual(self.run_script().returncode, 3)
        (self.state / "status").unlink()
        (self.state / "device_token").unlink()
        self.assertEqual(self.run_script().returncode, 1)
        self.assertFalse((self.fake / "calls").exists())

    def test_start_adds_one_cron_line_stop_removes_it(self):
        self.run_script("start")
        self.run_script("start")
        self.assertEqual(self.cron.read_text().count("hams-node-report run"), 1)
        self.run_script("stop")
        self.assertNotIn("hams-node-report", self.cron.read_text())


if __name__ == "__main__":
    unittest.main()
