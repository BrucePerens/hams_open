# SPDX-License-Identifier: GPL-2.0-or-later
"""Removing a package must take its cron line out of the crontab (an orphaned line runs a missing program every few minutes).

Each package's prerm block is read from pkg/<name>.mk, pointed at the repo's own script and a scratch crontab, and run with sh.
"""

import pathlib
import re
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
PKG = HERE.parent / "pkg"
SBIN = HERE.parent / "files" / "usr" / "sbin"
OTHER = "3 3 12 12 * /usr/bin/nginx-util 'check_ssl'"
# package -> the cron line its own scripts add
LINES = {
    "report": "*/30 * * * * /usr/sbin/hams-node-report run",
    "traffic": "7 * * * * /usr/sbin/hams-node-traffic run",
    "update": "43 4 * * * /usr/sbin/hams-node-update run",
    "status": "*/2 * * * * /usr/sbin/hams-node-status run\n*/2 * * * * /usr/sbin/hams-node-status gossip",
    "enroll": "17 */6 * * * /usr/sbin/hams-node-enroll fetch",
}


def prerm(name):
    text = (PKG / ("hams-node-%s.mk" % name)).read_text()
    found = re.search(r"^define Package/hams-node-%s/prerm\n(.*?)^endef" % name, text, re.S | re.M)
    return found.group(1).replace("$$", "$") if found else None


class PrermCron(unittest.TestCase):
    def test_prerm_removes_only_its_own_cron_line(self):
        for name, lines in LINES.items():
            with self.subTest(package=name), tempfile.TemporaryDirectory() as tmp:
                body = prerm(name)
                self.assertIsNotNone(body, "hams-node-%s has no prerm" % name)
                cron = pathlib.Path(tmp) / "root"
                cron.write_text(OTHER + "\n" + lines + "\n")
                script = body.replace("/etc/crontabs/root", str(cron))
                for tool in ("report", "traffic", "update", "status"):
                    script = script.replace("/usr/sbin/hams-node-%s " % tool, "%s/hams-node-%s -c %s %s " % (SBIN, tool, cron, "" if tool == "update" else "-s %s/state" % tmp))
                subprocess.run(["sh", "-c", script], check=True, env={"PATH": "/usr/bin:/bin"})
                self.assertEqual(cron.read_text(), OTHER + "\n")


if __name__ == "__main__":
    unittest.main()
