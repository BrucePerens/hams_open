# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free tests of the broker's provisioning script (secret_broker/provision.py), added to kill the mutation
survivors of the second mutation pass (2026-11-13): the unit file and the plan are pinned exactly (they are the broker's
sandbox and the file modes of its secrets), and `apply` is run against a temporary tree with `chown` recorded."""
import contextlib
import io
import json
import os
import shutil
import stat
import tempfile

from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.tests.common import SafePatchMixin
from odoo.addons.zero_sudo.daemon.secret_broker import provision

UNIT = """[Unit]
Description=hams secret broker (Tier B: operations on secrets the web process cannot read)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=hams_secret_broker
Group=hams_secret_broker_clients
ExecStart=/usr/bin/python3 /opt/zs/daemon/secret_broker/main.py --config /etc/hams_secret_broker/broker.json
LoadCredential=secrets:/etc/hams_secret_broker/secrets
RuntimeDirectory=hams_secret_broker
RuntimeDirectoryMode=0750
UMask=0117
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
CapabilityBoundingSet=
SystemCallArchitectures=native
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
"""


def _capture(function, *args, **kwargs):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        result = function(*args, **kwargs)
    return result, out.getvalue(), err.getvalue()


@tagged("post_install", "-at_install")
class TestProvisionPlan(BaseCase):
    # Tests [@ANCHOR: zero_sudo:secret_broker_provision_unit_text]
    # Tests [@ANCHOR: zero_sudo:secret_broker_provision_odoo_dropin_text]
    # Tests [@ANCHOR: zero_sudo:secret_broker_provision_default_config]
    # Tests [@ANCHOR: zero_sudo:secret_broker_provision_plan]

    def test_the_unit_text_is_exactly_this(self):
        self.assertEqual(provision.unit_text("/opt/zs/daemon", "/usr/bin/python3"), UNIT)
        self.assertEqual(provision.unit_text("/opt/zs/daemon"), UNIT)
        self.assertIn("ExecStart=/usr/bin/python3.11 /x/secret_broker/main.py", provision.unit_text("/x", "/usr/bin/python3.11"))

    def test_the_dropin_adds_only_the_clients_group(self):
        self.assertEqual(provision.odoo_dropin_text(), "[Service]\nSupplementaryGroups=hams_secret_broker_clients\n")

    def test_the_default_config(self):
        self.assertEqual(provision.default_config("www"), {
            "socket": "/run/hams_secret_broker/broker.sock", "allowed_users": ["www"], "deny_readers": ["www"],
            "credentials": "systemd", "secrets": {}})

    def test_the_plan_is_exactly_this(self):
        steps = provision.plan("www", "/opt/zs/daemon", "odoo.service")
        self.assertEqual(steps, [
            ("run", ["groupadd", "--system", "--force", "hams_secret_broker_clients"]),
            ("run_if_no_user", ["useradd", "--system", "--no-create-home", "--shell", "/usr/sbin/nologin",
                                "--gid", "hams_secret_broker_clients", "hams_secret_broker"]),
            ("run", ["usermod", "--append", "--groups", "hams_secret_broker_clients", "www"]),
            ("dir", "/etc/hams_secret_broker", 0o755),
            ("file_if_absent", "/etc/hams_secret_broker/broker.json", 0o644,
             json.dumps(provision.default_config("www"), indent=2) + "\n"),
            ("file_if_absent", "/etc/hams_secret_broker/secrets", 0o600,
             "# name=value, one per line. Root-only; the broker gets it through LoadCredential=.\n"),
            ("file", "/etc/systemd/system/hams-secret-broker.service", 0o644, UNIT),
            ("file", "/etc/systemd/system/odoo.service.d/hams_secret_broker.conf", 0o644,
             "[Service]\nSupplementaryGroups=hams_secret_broker_clients\n"),
            ("run", ["systemctl", "daemon-reload"]),
        ])
        self.assertEqual(provision.plan("www", "/opt/zs/daemon")[:-1], steps[:-2])  # no odoo unit: no drop-in
        self.assertEqual(provision.plan("www", "/opt/zs/daemon", None, "/usr/bin/python3.12")[6][3],
                         UNIT.replace("ExecStart=/usr/bin/python3 ", "ExecStart=/usr/bin/python3.12 "))
        self.assertEqual(provision.plan("www", "/opt/zs/daemon", "")[-1], ("run", ["systemctl", "daemon-reload"]))
        self.assertEqual(len(provision.plan("www", "/opt/zs/daemon", "")), 8)
        self.assertEqual(provision.plan("www", "/c")[6][3].count("ExecStart=/usr/bin/python3 /c/secret_broker/main.py"), 1)


@tagged("post_install", "-at_install")
class TestProvisionApply(SafePatchMixin, BaseCase):
    # Tests [@ANCHOR: zero_sudo:secret_broker_provision_account_exists]
    # Tests [@ANCHOR: zero_sudo:secret_broker_provision_apply]
    # Tests [@ANCHOR: zero_sudo:secret_broker_provision_main]

    def setUp(self):
        super().setUp()
        self.root = tempfile.mkdtemp(prefix="provision_units_")
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        self.chowns, self.runs, self.flags = [], [], []
        self.safe_patch_object(provision.os, "chown", new=lambda path, uid, gid: self.chowns.append((path, uid, gid)))
        real_open = os.open
        self.safe_patch_object(provision.os, "open", new=lambda path, flags, mode=0o777: self.flags.append(flags) or real_open(path, flags, mode))

    def _run(self, command, check=False):
        self.runs.append((command, check))

    def _apply(self, steps, dry_run=False, exists=lambda name: False):
        return _capture(provision.apply, steps, dry_run, run=self._run, user_exists=exists)

    def test_run_steps_print_and_run_with_check(self):
        _, out, _ = self._apply([("run", ["a", "b"]), ("run_if_no_user", ["useradd", "x", "svc"])])
        self.assertEqual(out, "run: a b\nrun: useradd x svc\n")
        self.assertEqual(self.runs, [(["a", "b"], True), (["useradd", "x", "svc"], True)])

    def test_an_existing_account_is_kept(self):
        asked = []
        _, out, _ = self._apply([("run_if_no_user", ["useradd", "x", "svc"]), ("run", ["after"])],
                                exists=lambda name: asked.append(name) or True)
        self.assertEqual(out, "keep: account svc exists\nrun: after\n")
        self.assertEqual(asked, ["svc"])
        self.assertEqual(self.runs, [(["after"], True)])

    def test_a_dry_run_prints_and_changes_nothing(self):
        target = os.path.join(self.root, "d")
        steps = [("run", ["a"]), ("run_if_no_user", ["useradd", "svc"]), ("dir", target, 0o755),
                 ("file", os.path.join(target, "f"), 0o600, "x")]
        _, out, _ = self._apply(steps, dry_run=True)
        self.assertEqual(out, "run: a\nrun: useradd svc\ndir: %s root:root 0o755\nwrite: %s root:root 0o600\n" % (target, os.path.join(target, "f")))
        self.assertEqual((self.runs, self.chowns, self.flags, os.listdir(self.root)), ([], [], [], []))

    def test_a_dir_step_creates_chowns_to_root_and_sets_the_mode(self):
        target = os.path.join(self.root, "etc", "broker")
        _, out, _ = self._apply([("dir", target, 0o750)])
        self.assertEqual(out, "dir: %s root:root 0o750\n" % target)
        self.assertEqual(stat.S_IMODE(os.stat(target).st_mode), 0o750)
        self.assertEqual(self.chowns, [(target, 0, 0)])
        self._apply([("dir", target, 0o755)])  # an existing directory is fine
        self.assertEqual(stat.S_IMODE(os.stat(target).st_mode), 0o755)

    def test_a_file_is_written_with_its_mode_and_owner(self):
        path = os.path.join(self.root, "sub", "f.conf")
        _, out, _ = self._apply([("file_if_absent", path, 0o600, "secret text\n")])
        self.assertEqual(out, "write: %s root:root 0o600\n" % path)
        with open(path, encoding="utf-8") as handle:
            self.assertEqual(handle.read(), "secret text\n")
        self.assertEqual(stat.S_IMODE(os.stat(path).st_mode), 0o600)
        self.assertEqual(self.chowns, [(path, 0, 0)])
        self.assertTrue(self.flags[0] & os.O_EXCL and self.flags[0] & os.O_NOFOLLOW and self.flags[0] & os.O_CREAT)
        self.assertFalse(self.flags[0] & os.O_TRUNC)

    def test_an_existing_file_if_absent_is_left_alone_even_a_dangling_link(self):
        path = os.path.join(self.root, "f")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("operator's text")
        dangling = os.path.join(self.root, "link")
        os.symlink(os.path.join(self.root, "nowhere"), dangling)
        _, out, _ = self._apply([("file_if_absent", path, 0o600, "new"), ("file_if_absent", dangling, 0o600, "new")])
        self.assertEqual(out, "keep: %s exists\nkeep: %s exists\n" % (path, dangling))
        with open(path, encoding="utf-8") as handle:
            self.assertEqual(handle.read(), "operator's text")
        self.assertEqual((self.chowns, self.flags), ([], []))

    def test_a_file_step_rewrites_and_truncates(self):
        path = os.path.join(self.root, "unit")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("old and much longer text")
        os.chmod(path, 0o666)
        self._apply([("file", path, 0o644, "new")])
        with open(path, encoding="utf-8") as handle:
            self.assertEqual(handle.read(), "new")
        self.assertEqual(stat.S_IMODE(os.stat(path).st_mode), 0o644)
        self.assertTrue(self.flags[0] & os.O_TRUNC)
        self.assertFalse(self.flags[0] & os.O_EXCL)
        self.assertTrue(self.flags[0] & os.O_NOFOLLOW)

    def test_account_exists_asks_the_system(self):
        import pwd
        self.assertTrue(provision.account_exists(pwd.getpwall()[0].pw_name))
        self.assertFalse(provision.account_exists("no_such_account_for_unit_test_x1"))

    def test_main_refuses_to_change_anything_unless_root(self):
        applied = []
        self.safe_patch_object(provision, "apply", new=lambda steps, dry_run: applied.append((steps, dry_run)))
        self.safe_patch_object(provision.os, "geteuid", new=lambda: 1000)
        code, out, err = _capture(provision.main, ["--web-user", "www", "--code-dir", "/opt/zs"])
        self.assertEqual((code, out, err, applied), (2, "", "run as root (or use --dry-run)\n", []))
        self.safe_patch_object(provision.os, "geteuid", new=lambda: 1)
        self.assertEqual(_capture(provision.main, ["--web-user", "www", "--code-dir", "/opt/zs"])[0], 2)
        code, out, err = _capture(provision.main, ["--web-user", "www", "--code-dir", "/opt/zs", "--dry-run"])
        self.assertEqual((code, err), (0, ""))
        self.assertEqual(applied, [(provision.plan("www", "/opt/zs", None, "/usr/bin/python3"), True)])

    def test_main_as_root_applies_the_plan_for_real_arguments(self):
        applied = []
        self.safe_patch_object(provision, "apply", new=lambda steps, dry_run: applied.append((steps, dry_run)))
        self.safe_patch_object(provision.os, "geteuid", new=lambda: 0)
        code = _capture(provision.main, ["--web-user", "www", "--code-dir", "rel/dir", "--odoo-unit", "odoo.service",
                                         "--python", "/usr/bin/python3.12"])[0]
        self.assertEqual(code, 0)
        self.assertEqual(applied, [(provision.plan("www", os.path.abspath("rel/dir"), "odoo.service", "/usr/bin/python3.12"), False)])

    def test_main_requires_the_web_user_and_the_code_dir(self):
        for argv in ([], ["--web-user", "www"], ["--code-dir", "/x"]):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as stop:
                provision.main(argv)
            self.assertEqual(stop.exception.code, 2)

    def test_the_help_text(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), self.assertRaises(SystemExit):
            provision.main(["--help"])
        text = " ".join(out.getvalue().split())
        for expected in ("Provision the Tier B secret broker", "the account the Odoo web process runs as",
                         "the zero_sudo/daemon directory of the installed module",
                         "the Odoo systemd unit to give a drop-in, e.g. odoo.service"):
            self.assertIn(expected, text)
        self.assertNotIn("XX", text)
