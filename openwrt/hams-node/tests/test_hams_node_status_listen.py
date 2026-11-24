# SPDX-License-Identifier: GPL-2.0-or-later
"""hams-node-status listen: the gossip listener on tcp 5710 hands each neighbour to a bounded `serve`, as separate words after nc -e."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent.parent / "files" / "usr" / "sbin" / "hams-node-status"


class ListenTests(unittest.TestCase):
    def test_nc_gets_the_serve_command_as_words_under_a_timeout(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "nc").write_text('#!/bin/sh\nfor a in "$@"; do echo "$a"; done > "%s/args"\n' % root)
            (root / "nc").chmod(0o755)
            env = dict(os.environ, PATH="%s:%s" % (root, os.environ["PATH"]))
            subprocess.run(["sh", str(SCRIPT), "-s", str(root / "state"), "listen"], env=env, check=True, capture_output=True, timeout=30)
            args = (root / "args").read_text().splitlines()
            self.assertEqual(args[:4], ["-ll", "-p", "5710", "-e"])
            self.assertEqual(args[4:6], ["timeout", "10"], "a connection that never sends its line must not hold the process forever")
            self.assertEqual(args[6:], ["/usr/sbin/hams-node-status", "-s", str(root / "state"), "serve"])


if __name__ == "__main__":
    unittest.main()
