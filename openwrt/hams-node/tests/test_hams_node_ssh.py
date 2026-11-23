# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-ssh with a stand-in uci that records what is set."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-ssh"
UCI = """#!/bin/sh
case "$*" in
"-q show dropbear") printf 'dropbear.cfg01=dropbear\\ndropbear.cfg01.Port=22\\ndropbear.cfg02=dropbear\\n' ;;
"-q get dropbear.cfg01.PasswordAuth") echo on ;;
"-q get dropbear.cfg01.RootPasswordAuth") echo on ;;
*) echo "$*" >> "$FAKE/uci" ;;
esac
"""


class SshTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.state = self.root / "state"
        self.state.mkdir()
        self.keys = self.root / "authorized_keys"
        bindir = self.root / "bin"
        bindir.mkdir()
        (bindir / "uci").write_text(UCI)
        (bindir / "uci").chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.root), PATH="%s:%s" % (bindir, os.environ["PATH"]))

    def run_script(self, cmd):
        return subprocess.run(["sh", str(SCRIPT), "-s", str(self.state), "-k", str(self.keys), cmd], env=self.env, capture_output=True, text=True, timeout=60)

    def uci(self):
        path = self.root / "uci"
        return path.read_text() if path.exists() else ""

    def enrol_with_key(self):
        (self.state / "device_token").write_text("tok\n")
        self.keys.write_text("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 owner@laptop\n")

    def test_enrolled_with_a_key_turns_both_password_settings_off_on_every_instance(self):
        self.enrol_with_key()
        self.assertEqual(self.run_script("apply").returncode, 0)
        uci = self.uci()
        for section in ("cfg01", "cfg02"):
            self.assertIn("set dropbear.%s.PasswordAuth=off" % section, uci)
            self.assertIn("set dropbear.%s.RootPasswordAuth=off" % section, uci)
        self.assertIn("commit dropbear", uci)

    def test_no_key_installed_keeps_password_login_so_the_owner_is_not_locked_out(self):
        (self.state / "device_token").write_text("tok\n")
        self.assertEqual(self.run_script("apply").returncode, 0)
        self.assertEqual(self.uci(), "")

    def test_not_enrolled_changes_nothing(self):
        self.keys.write_text("ssh-ed25519 AAAA x\n")
        self.assertEqual(self.run_script("apply").returncode, 0)
        self.assertEqual(self.uci(), "")

    def test_a_non_key_line_does_not_count_as_a_key(self):
        (self.state / "device_token").write_text("tok\n")
        self.keys.write_text("# a comment ssh-ed25519\n")
        self.run_script("apply")
        self.assertEqual(self.uci(), "")

    def test_off_restores_the_saved_settings_and_deletes_unset_ones(self):
        self.enrol_with_key()
        self.run_script("apply")
        (self.root / "uci").unlink()
        self.assertEqual(self.run_script("off").returncode, 0)
        uci = self.uci()
        self.assertIn("set dropbear.cfg01.PasswordAuth=on", uci)
        self.assertIn("set dropbear.cfg01.RootPasswordAuth=on", uci)
        self.assertIn("delete dropbear.cfg02.PasswordAuth", uci)
        self.assertFalse((self.state / "ssh.orig").exists())

    def test_applying_twice_keeps_the_first_saved_settings(self):
        self.enrol_with_key()
        self.run_script("apply")
        self.run_script("apply")
        self.assertEqual(len((self.state / "ssh.orig").read_text().splitlines()), 2)
        self.assertIn("cfg01 on on", (self.state / "ssh.orig").read_text())


if __name__ == "__main__":
    unittest.main()
