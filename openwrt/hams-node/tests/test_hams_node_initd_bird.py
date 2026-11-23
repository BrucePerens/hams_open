# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the boot-time init scripts of hams-node-bird: sourced with procd stand-ins and recording tools."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
INITD = HERE.parent / "files" / "etc" / "init.d"
HARNESS = """
procd_open_instance() { echo "procd_open_instance" >> "$FAKE/log"; }
procd_close_instance() { echo "procd_close_instance" >> "$FAKE/log"; }
procd_set_param() { echo "procd_set_param $*" >> "$FAKE/log"; }
procd_add_reload_trigger() { echo "trigger $*" >> "$FAKE/log"; }
. "$SCRIPT"
"$@"
"""


class InitTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()

    def tool(self, name, body):
        p = self.root / "bin" / name
        p.write_text("#!/bin/sh\n" + body)
        p.chmod(0o755)

    def run_fn(self, script, fn, **env):
        env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root), SCRIPT=str(INITD / script),
                   HAMS_NODE_ETC=str(self.root / "etc"), HAMS_NODE_DNS_OUT=str(self.root / "dns" / "hams.conf"), **env)
        (self.root / "log").unlink(missing_ok=True)
        result = subprocess.run(["sh", "-c", HARNESS, "sh", fn], env=env, capture_output=True, text=True)
        log = self.root / "log"
        return result, log.read_text().splitlines() if log.exists() else []

    def test_bird_checks_then_runs_its_own_instance(self):
        self.tool("hams-node-bird", 'echo "router id 1.2.3.4;" > "$2"\n')
        self.tool("bird", 'echo "bird $*" >> "$FAKE/log"\n')
        result, log = self.run_fn("hams-node-bird", "start_service")
        self.assertEqual(result.returncode, 0, result.stderr)
        conf = self.root / "etc" / "bird.conf"
        self.assertEqual(log[0], "bird -p -c %s" % conf)  # parse check first
        self.assertIn("procd_set_param command /usr/sbin/bird -f -c %s -s /var/run/hams-node-bird.ctl" % conf, log)

    def test_bird_does_not_run_when_nothing_rendered_and_drops_a_stale_file(self):
        self.tool("hams-node-bird", "exit 0\n")
        self.tool("bird", 'echo "bird $*" >> "$FAKE/log"\n')
        (self.root / "etc").mkdir()
        (self.root / "etc" / "bird.conf").write_text("stale")
        result, log = self.run_fn("hams-node-bird", "start_service")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(log, [])
        self.assertFalse((self.root / "etc" / "bird.conf").exists())

    def test_bird_not_started_on_a_config_that_does_not_parse(self):
        self.tool("hams-node-bird", 'echo bad > "$2"\n')
        self.tool("bird", "exit 1\n")
        result, log = self.run_fn("hams-node-bird", "start_service")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("procd_open_instance", log)

    def test_trigger_and_procd(self):
        result, log = self.run_fn("hams-node-bird", "service_triggers")
        self.assertEqual(log, ["trigger hams_node_bird"])
        self.assertIn("USE_PROCD=1", (INITD / "hams-node-bird").read_text())


if __name__ == "__main__":
    unittest.main()
