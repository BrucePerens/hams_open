# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-traffic with stand-ins for wg (canned `show IFACE transfer`), uclient-fetch (one canned reply, request logged) and jsonfilter."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-traffic"
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
# the real wg prints more subcommands (endpoints, allowed-ips); the script must ask only for `transfer`
WG = """#!/bin/sh
echo "$@" >> "$FAKE/wg_args"
cat "$FAKE/transfer"
"""
KEY_A = "A" * 43 + "="
KEY_B = "B" * 43 + "="


class TrafficTests(unittest.TestCase):
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
        bindir = self.root / "bin"
        bindir.mkdir()
        for name, text in (("uclient-fetch", FETCH), ("wg", WG)):
            (bindir / name).write_text(text)
            (bindir / name).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s:%s" % (bindir, HERE, os.environ["PATH"]))
        self.reply("success")

    def transfer(self, a=(0, 0), b=(0, 0)):
        (self.fake / "transfer").write_text("%s\t%d\t%d\n%s\t%d\t%d\n" % (KEY_A, a[0], a[1], KEY_B, b[0], b[1]))

    def reply(self, status):
        (self.fake / "reply").write_text(json.dumps({"jsonrpc": "2.0", "id": None, "result": {"status": status}}))

    def run_script(self, command="run"):
        return subprocess.run(["sh", str(SCRIPT), "-u", "https://portal.example", "-s", str(self.state), "-c", str(self.cron), "-i", "wgtest", command],
                              env=self.env, capture_output=True, text=True, timeout=60)

    def calls(self):
        path = self.fake / "calls"
        return [json.loads(line.split(" --post-data=", 1)[1]) for line in path.read_text().splitlines()] if path.exists() else []

    def baseline_then(self, first, second):
        self.transfer(*first)
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(self.calls(), [])
        self.transfer(*second)
        return self.run_script()

    def test_first_reading_is_only_a_baseline_then_reports_the_change(self):
        result = self.baseline_then(((100, 200), (5, 5)), ((1100, 700), (5, 5)))
        self.assertEqual(result.returncode, 0, result.stderr)
        call = self.calls()[0]
        self.assertEqual(call["method"], "call")
        params = call["params"]
        self.assertEqual(params["token"], "tok_en-1")
        self.assertEqual(params["peers"], [{"peer": KEY_A, "rx": 1000, "tx": 500}])
        self.assertIsInstance(params["interval"], int)

    def test_payload_has_only_the_aggregate_fields(self):
        self.baseline_then(((0, 0), (0, 0)), ((10, 20), (30, 40)))
        params = self.calls()[0]["params"]
        self.assertEqual(set(params), {"token", "interval", "peers"})
        for peer in params["peers"]:
            self.assertEqual(set(peer), {"peer", "rx", "tx"})
        self.assertEqual((self.fake / "wg_args").read_text().split("\n")[0], "show wgtest transfer")

    def test_a_restarted_counter_counts_from_zero_and_large_counts_survive(self):
        result = self.baseline_then(((9_000_000_000, 9_000_000_000), (0, 0)), ((7, 3), (0, 0)))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls()[0]["params"]["peers"], [{"peer": KEY_A, "rx": 7, "tx": 3}])
        self.transfer((7 + 6_000_000_000, 3), (0, 0))
        self.run_script()
        self.assertEqual(self.calls()[1]["params"]["peers"], [{"peer": KEY_A, "rx": 6_000_000_000, "tx": 0}])

    def test_no_change_sends_nothing(self):
        result = self.baseline_then(((1, 1), (2, 2)), ((1, 1), (2, 2)))
        self.assertEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [])

    def test_a_failed_post_keeps_the_old_baseline_so_nothing_is_lost(self):
        self.baseline_then(((0, 0), (0, 0)), ((10, 0), (0, 0)))
        (self.fake / "calls").unlink()
        (self.fake / "reply").unlink()
        self.transfer((50, 0), (0, 0))
        self.assertEqual(self.run_script().returncode, 4)
        self.reply("success")
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(self.calls()[-1]["params"]["peers"], [{"peer": KEY_A, "rx": 40, "tx": 0}])

    def test_opt_out_reports_nothing_and_opt_in_resumes(self):
        self.transfer()
        self.run_script()
        self.run_script("optout")
        self.transfer((10, 10), (0, 0))
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(self.calls(), [])
        self.run_script("optin")
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual(len(self.calls()), 1)

    def test_nothing_but_the_one_request_leaves_the_node_and_no_log_has_peer_data(self):
        self.baseline_then(((0, 0), (0, 0)), ((10, 20), (0, 0)))
        result = self.run_script()
        self.assertEqual(result.stdout + result.stderr, "")
        self.assertEqual(len(self.calls()), 1)

    def test_refused_exits_1_unreachable_4_revoked_3_not_enrolled_1(self):
        self.baseline_then(((0, 0), (0, 0)), ((10, 0), (0, 0)))
        self.reply("error")
        self.transfer((20, 0), (0, 0))
        self.assertEqual(self.run_script().returncode, 1)
        (self.state / "status").write_text("revoked\n")
        self.assertEqual(self.run_script().returncode, 3)
        (self.state / "status").unlink()
        (self.state / "device_token").unlink()
        self.assertEqual(self.run_script().returncode, 1)

    def test_start_adds_one_cron_line_stop_removes_it(self):
        self.run_script("start")
        self.run_script("start")
        self.assertEqual(self.cron.read_text().count("hams-node-traffic run"), 1)
        self.run_script("stop")
        self.assertNotIn("hams-node-traffic", self.cron.read_text())


if __name__ == "__main__":
    unittest.main()
