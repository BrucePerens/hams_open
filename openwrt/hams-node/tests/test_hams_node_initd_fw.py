# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the boot-time init scripts of hams-node-fw: sourced with procd stand-ins and recording tools."""

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
                   HAMS_NODE_ETC=str(self.root / "etc"), HAMS_NODE_INITD=str(self.root / "bin"), HAMS_NODE_DNS_OUT=str(self.root / "dns" / "hams.conf"), **env)
        (self.root / "log").unlink(missing_ok=True)
        result = subprocess.run(["sh", "-c", HARNESS, "sh", fn], env=env, capture_output=True, text=True)
        log = self.root / "log"
        return result, log.read_text().splitlines() if log.exists() else []

    def test_fw_applies(self):
        self.tool("hams-node-fw", 'echo "fw $*" >> "$FAKE/log"\n')
        result, log = self.run_fn("hams-node-fw", "start_service")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, ["fw apply"])

    def test_trigger_and_procd(self):
        result, log = self.run_fn("hams-node-fw", "service_triggers")
        self.assertEqual(log, ["trigger hams_node_fw"])
        self.assertIn("USE_PROCD=1", (INITD / "hams-node-fw").read_text())

    def test_stop_removes_zones_and_reloads_firewall(self):
        self.tool("hams-node-fw", 'echo "fw $*" >> "$FAKE/log"\n')
        self.tool("firewall", 'echo "firewall $*" >> "$FAKE/log"\n')
        result, log = self.run_fn("hams-node-fw", "stop_service")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, ["fw remove", "firewall reload"])

    def test_prerm_removes_zones_only_on_package_removal(self):
        mk = (HERE.parent / "pkg" / "hams-node-fw.mk").read_text()
        prerm = mk.split("define Package/hams-node-fw/prerm", 1)[1].split("endef", 1)[0]
        self.assertIn("/usr/sbin/hams-node-fw remove", prerm)
        self.assertIn('"$$1" != remove', prerm)


if __name__ == "__main__":
    unittest.main()
