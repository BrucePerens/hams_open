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

    def member_keys(self, text):
        (self.state / "device_token").write_text("tok\n")
        (self.state / "ssh_keys").write_text(text)

    def test_exported_member_certificate_keys_are_authorized_and_make_ssh_key_only(self):
        self.member_keys("1a2b ecdsa-sha2-nistp256 AAAAE2VjZHNh\nBEEF ecdsa-sha2-nistp256 BBBB\n")
        self.keys.write_text("ssh-ed25519 OWNKEY mine\n")
        self.assertEqual(self.run_script("apply").returncode, 0)
        lines = self.keys.read_text().splitlines()
        self.assertEqual(lines[0], "ssh-ed25519 OWNKEY mine")
        self.assertIn("ecdsa-sha2-nistp256 AAAAE2VjZHNh hams-cert-1a2b", lines)
        self.assertIn("set dropbear.cfg01.PasswordAuth=off", self.uci())

    def test_member_keys_alone_are_enough_and_other_key_types_or_junk_are_ignored(self):
        self.member_keys("1 ssh-rsa AAAAB3\n2 ecdsa-sha2-nistp256 CCCC\n3 ecdsa-sha2-nistp256 DD;DD\n")
        self.assertEqual(self.run_script("apply").returncode, 0)
        text = self.keys.read_text()
        self.assertIn("ecdsa-sha2-nistp256 CCCC hams-cert-2", text)
        self.assertNotIn("ssh-rsa", text)
        self.assertNotIn("DD;DD", text)
        self.assertIn("set dropbear.cfg01.PasswordAuth=off", self.uci())

    def test_a_revoked_certificate_on_the_deprecation_list_is_left_out_and_reapplying_replaces_the_block(self):
        self.member_keys("1a2b ecdsa-sha2-nistp256 AAAA\n3c ecdsa-sha2-nistp256 BBBB\n")
        self.run_script("apply")
        (self.state / "deprecation_list").write_text("cert:ISSUERHASH:0001A2B \n")
        self.run_script("apply")
        text = self.keys.read_text()
        self.assertNotIn("AAAA", text)
        self.assertIn("BBBB hams-cert-3c", text)
        self.assertEqual(text.count("# hams-node begin"), 1)

    def test_revoking_the_last_key_leaves_password_login_off_and_ssh_refusing_every_login(self):
        self.member_keys("1a2b ecdsa-sha2-nistp256 AAAA\n")
        self.run_script("apply")
        (self.root / "uci").unlink()
        (self.state / "deprecation_list").write_text("cert:ISSUERHASH:1a2b \n")
        self.assertEqual(self.run_script("apply").returncode, 0)
        self.assertNotIn("PasswordAuth=on", self.uci())
        self.assertNotIn("ecdsa", self.keys.read_text())
        self.assertTrue((self.state / "ssh.orig").exists(), "passwords stay off; only `off` (unenrol) restores them")

    def test_off_removes_the_block_but_keeps_the_owners_own_keys(self):
        self.member_keys("1a2b ecdsa-sha2-nistp256 AAAA\n")
        self.keys.write_text("ssh-ed25519 OWNKEY mine\n")
        self.run_script("apply")
        self.assertEqual(self.run_script("off").returncode, 0)
        self.assertEqual(self.keys.read_text(), "ssh-ed25519 OWNKEY mine\n")
        self.assertFalse((self.state / "ssh_keys").exists())


if __name__ == "__main__":
    unittest.main()
