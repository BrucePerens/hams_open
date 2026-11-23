# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-time: ordered clock sources, cross-checking, no wild steps. Stand-ins for uci and date; sources are shell commands."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-time"
UCI = '#!/bin/sh\nkey=${3#hams_node_time.main.}\ngrep "^$key=" "$FAKE/uci" | head -n 1 | cut -d= -f2-\n'
NOW = 1800000000  # a clock in 2027


class TimeTests(unittest.TestCase):
    def run_time(self, sources, now=NOW, **over):
        """sources: ordered list of (name, output text or None for a dead source)."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        (root / "bin").mkdir()
        (root / "bin" / "uci").write_text(UCI)
        (root / "bin" / "date").write_text('#!/bin/sh\ncase $1 in -s) echo "$2" > "$FAKE/set";; +%%s) echo %d;; esac\n' % now)
        for n in ("uci", "date"):
            (root / "bin" / n).chmod(0o755)
        (root / "src").mkdir()
        cmds = []
        for name, out in sources:
            script = root / "src" / name
            script.write_text("#!/bin/sh\n%s\n" % ("echo %s" % out if out is not None else "exit 1"))
            script.chmod(0o755)
            cmds.append("%s=%s" % (name, script))
        cfg = dict({"enabled": "1", "source": " ".join(cmds), "status": str(root / "status")}, **over)
        (root / "uci").write_text("".join("%s=%s\n" % kv for kv in cfg.items()))
        env = dict(os.environ, PATH="%s:%s" % (root / "bin", os.environ["PATH"]), FAKE=str(root))
        result = subprocess.run(["sh", str(SCRIPT), "sync"], env=env, capture_output=True, text=True)
        stepped = (root / "set").read_text().strip() if (root / "set").exists() else None
        status = (root / "status").read_text() if (root / "status").exists() else ""
        return result, stepped, status

    def test_first_live_source_in_order_sets_a_small_correction(self):
        _, stepped, status = self.run_time([("ntp", None), ("hamntp", "%d 0.2" % (NOW + 3)), ("gps", "%d 1" % (NOW + 3))])
        self.assertEqual(stepped, "@%d" % (NOW + 3))
        self.assertIn("source=hamntp", status)

    def test_no_source_leaves_the_clock_alone(self):
        result, stepped, status = self.run_time([("ntp", None), ("gps", None)])
        self.assertEqual(result.returncode, 0)
        self.assertIsNone(stepped)
        self.assertIn("source=none", status)

    def test_disagreeing_sources_are_flagged_and_nothing_is_stepped(self):
        _, stepped, status = self.run_time([("ntp", "%d 0.1" % (NOW + 3)), ("gps", "%d 1" % (NOW + 9000))])
        self.assertIsNone(stepped)
        self.assertIn("disagree=ntp,gps", status)

    def test_large_forward_jump_from_one_source_is_refused(self):
        _, stepped, status = self.run_time([("gps", "%d 1" % (NOW + 86400 * 400))])  # spoofed GPS: a year ahead
        self.assertIsNone(stepped)
        self.assertIn("refused=jump", status)

    def test_large_forward_jump_needs_a_second_agreeing_source(self):
        t = NOW + 86400 * 2
        _, stepped, _ = self.run_time([("ntp", "%d 0.1" % t), ("gps", "%d 1" % (t + 1))])
        self.assertEqual(stepped, "@%d" % t)

    def test_backward_step_beyond_threshold_is_refused_from_one_source(self):
        _, stepped, status = self.run_time([("gps", "%d 1" % (NOW - 60))])
        self.assertIsNone(stepped)
        self.assertIn("refused=backward", status)

    def test_small_backward_step_is_allowed(self):
        _, stepped, _ = self.run_time([("gps", "%d 1" % (NOW - 2))])
        self.assertEqual(stepped, "@%d" % (NOW - 2))

    def test_a_clock_before_the_floor_takes_the_first_source(self):
        _, stepped, _ = self.run_time([("gps", "%d 1" % NOW)], now=86400)  # RTC dead, clock at 1970
        self.assertEqual(stepped, "@%d" % NOW)

    def test_bad_source_output_is_ignored(self):
        _, stepped, status = self.run_time([("gps", "garbage; rm -rf"), ("rtc", "%d 2" % (NOW + 1))])
        self.assertEqual(stepped, "@%d" % (NOW + 1))
        self.assertIn("source=rtc", status)

    def test_disabled_does_nothing(self):
        _, stepped, status = self.run_time([("ntp", "%d 0.1" % (NOW + 1))], enabled="0")
        self.assertIsNone(stepped)
        self.assertEqual(status, "")


if __name__ == "__main__":
    unittest.main()
