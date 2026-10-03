# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import os
import stat
import tempfile
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.cloudflare.utils import cloudflare_systemd as cf_systemd


@tagged("post_install", "-at_install")
class TestCloudflareSystemd(HamsTransactionCase):
    """Pure unit coverage for cloudflare_systemd.py's own logic: subprocess.run (i.e. every
    `systemctl --user` call) is mocked throughout, the same "no real external process needed"
    approach test_wsgi_proxy_scheme.py already uses for its own env-less hook. See
    test_09/test_10 below for the two spots (secure file writing, unit rendering) that ARE
    exercised against a real filesystem, matching daemon_key_manager's own test convention of
    writing real files under /opt/hams/etc/keys/ rather than mocking the filesystem."""

    def _mock_run(self, returncode=0, stdout="", stderr=""):
        return self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_systemd.subprocess.run",
            return_value=MagicMock(returncode=returncode, stdout=stdout, stderr=stderr),
        )

    # Tests [@ANCHOR: cloudflare:is_tunnel_daemon_running]
    def test_01_is_tunnel_daemon_running_true_for_active_unit(self):
        mock_run = self._mock_run(returncode=0, stdout="active\n")
        self.assertTrue(cf_systemd.is_tunnel_daemon_running("cftun-test"))
        args = mock_run.call_args[0][0]
        self.assertEqual(args, ["systemctl", "--user", "is-active", "cloudflared@cftun-test.service"])

    def test_02_is_tunnel_daemon_running_false_for_inactive_unit(self):
        # `systemctl is-active` exits non-zero and prints "inactive"/"failed" for a
        # unit that isn't running -- both must read as not-running.
        self._mock_run(returncode=3, stdout="inactive\n")
        self.assertFalse(cf_systemd.is_tunnel_daemon_running("cftun-test"))

    def test_03_is_tunnel_daemon_running_false_when_systemctl_itself_fails(self):
        self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_systemd.subprocess.run",
            side_effect=OSError("systemctl not found"),
        )
        self.assertFalse(cf_systemd.is_tunnel_daemon_running("cftun-test"))

    # Tests [@ANCHOR: cloudflare:start_tunnel_daemon]
    def test_04_start_tunnel_daemon_writes_env_file_and_enables_unit(self):
        self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_systemd._ensure_unit_installed"
        )
        mock_run = self._mock_run(returncode=0, stdout="")
        try:
            result = cf_systemd.start_tunnel_daemon("fake-token-xyz", "cftun-write-test")
            self.assertTrue(result)
            env_path = cf_systemd._env_file_path("cftun-write-test")
            with open(env_path) as f:
                content = f.read()
            self.assertEqual(content.count("TUNNEL_TOKEN=fake-token-xyz"), 1)
            self.assertNotIn("CLOUDFLARE_TUNNEL_TOKEN", content)
            self.assertEqual(stat.S_IMODE(os.stat(env_path).st_mode), 0o600)
            call_args, call_kwargs = mock_run.call_args
            self.assertEqual(
                call_args[0],
                ["systemctl", "--user", "enable", "--now", "cloudflared@cftun-write-test.service"],
            )
            self.assertIn("XDG_RUNTIME_DIR", call_kwargs["env"])
        finally:
            path = cf_systemd._env_file_path("cftun-write-test")
            if os.path.exists(path):
                os.remove(path)

    def test_05_start_tunnel_daemon_returns_false_on_systemctl_failure(self):
        self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_systemd._ensure_unit_installed"
        )
        self._mock_run(returncode=1, stderr="Failed to enable unit")
        try:
            self.assertFalse(
                cf_systemd.start_tunnel_daemon("fake-token", "cftun-fail-test")
            )
        finally:
            path = cf_systemd._env_file_path("cftun-fail-test")
            if os.path.exists(path):
                os.remove(path)

    # Tests [@ANCHOR: cloudflare:stop_tunnel_daemon]
    def test_06_stop_tunnel_daemon_with_key_disables_that_unit_only(self):
        mock_run = self._mock_run(returncode=0)
        cf_systemd.stop_tunnel_daemon("cftun-stop-test")
        mock_run.assert_called_once()
        call_args, call_kwargs = mock_run.call_args
        self.assertEqual(
            call_args[0],
            ["systemctl", "--user", "disable", "--now", "cloudflared@cftun-stop-test.service"],
        )
        self.assertIn("XDG_RUNTIME_DIR", call_kwargs["env"])

    # Tests [@ANCHOR: cloudflare:stop_tunnel_daemon]
    def test_06b_stopping_a_tunnel_removes_its_run_token_file(self):
        # The file holds a live credential; start_tunnel_daemon writes it again on every start, so a stopped or
        # deleted tunnel has no use for it.
        path = cf_systemd._env_file_path("cftun-remove-test")
        cf_systemd._write_secure_file(path, "TUNNEL_TOKEN=secret\n", cf_systemd._KEYS_DIR)
        self.addCleanup(lambda: os.path.exists(path) and os.remove(path))
        self._mock_run(returncode=0)
        cf_systemd.stop_tunnel_daemon("cftun-remove-test")
        self.assertFalse(os.path.exists(path))

    # Tests [@ANCHOR: cloudflare:stop_tunnel_daemon]
    def test_06c_the_token_file_stays_when_the_unit_could_not_be_disabled(self):
        # A still-running unit with Restart=always needs the file to restart with.
        path = cf_systemd._env_file_path("cftun-keep-test")
        cf_systemd._write_secure_file(path, "TUNNEL_TOKEN=secret\n", cf_systemd._KEYS_DIR)
        self.addCleanup(lambda: os.path.exists(path) and os.remove(path))
        self._mock_run(returncode=1, stderr="Failed to disable unit")
        cf_systemd.stop_tunnel_daemon("cftun-keep-test")
        self.assertTrue(os.path.exists(path))

    # Tests [@ANCHOR: cloudflare:stop_tunnel_daemon]
    def test_06d_a_key_that_resolves_outside_the_keys_directory_is_never_removed(self):
        with tempfile.TemporaryDirectory() as tmp:
            keys_dir = os.path.join(tmp, "keys")
            os.makedirs(keys_dir)
            victim = os.path.join(tmp, "victim.env")
            with open(victim, "w") as f:
                f.write("x")
            self.safe_patch_object(cf_systemd, "_KEYS_DIR", keys_dir)
            self._mock_run(returncode=0)
            cf_systemd.stop_tunnel_daemon("../victim")
            self.assertTrue(os.path.exists(victim))

    # Tests [@ANCHOR: cloudflare:stop_tunnel_daemon]
    def test_06e_stopping_a_tunnel_that_has_no_token_file_is_quiet(self):
        self._mock_run(returncode=0)
        cf_systemd.stop_tunnel_daemon("cftun-never-started")  # must not raise

    def test_07_stop_tunnel_daemon_without_key_disables_every_discovered_unit(self):
        # No in-memory registry of "keys this process started" any more -- discovery goes
        # through `systemctl --user list-units`, which is authoritative regardless of which
        # Odoo worker (or none) last touched a given tunnel's unit.
        mock_run = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_systemd.subprocess.run",
            side_effect=[
                MagicMock(
                    returncode=0,
                    stdout="cloudflared@cftun-a.service loaded active running Cloudflare Tunnel (cftun-a)\n"
                    "cloudflared@cftun-b.service loaded active running Cloudflare Tunnel (cftun-b)\n",
                    stderr="",
                ),
                MagicMock(returncode=0, stdout="", stderr=""),
                MagicMock(returncode=0, stdout="", stderr=""),
            ],
        )
        cf_systemd.stop_tunnel_daemon()
        self.assertEqual(mock_run.call_count, 3)
        disabled = {call.args[0][-1] for call in mock_run.call_args_list[1:]}
        self.assertEqual(disabled, {"cloudflared@cftun-a.service", "cloudflared@cftun-b.service"})

    def test_08_stop_tunnel_daemon_without_key_is_a_no_op_when_list_units_fails(self):
        mock_run = self._mock_run(returncode=1, stderr="no such service manager")
        cf_systemd.stop_tunnel_daemon()
        mock_run.assert_called_once()

    # Tests [@ANCHOR: cloudflare:cloudflare_systemd] (secure file writing)
    def test_09_write_secure_file_refuses_paths_outside_the_mandatory_prefix(self):
        with self.assertRaises(ValueError):
            cf_systemd._write_secure_file(
                "/opt/hams/not-allowed.env", "content", cf_systemd._KEYS_DIR
            )

    def test_10_write_secure_file_writes_atomically_at_0600(self):
        path = os.path.join(cf_systemd._KEYS_DIR, "test_cloudflare_systemd_write.env")
        try:
            cf_systemd._write_secure_file(path, "FOO=bar\n", cf_systemd._KEYS_DIR)
            with open(path) as f:
                self.assertEqual(f.read(), "FOO=bar\n")
            self.assertEqual(stat.S_IMODE(os.stat(path).st_mode), 0o600)
            # No leftover temp files from the mkstemp-then-rename dance.
            leftovers = [
                f for f in os.listdir(cf_systemd._KEYS_DIR) if f.startswith(".cloudflared_")
            ]
            self.assertEqual(leftovers, [])
        finally:
            if os.path.exists(path):
                os.remove(path)

    # Tests [@ANCHOR: cloudflare:cloudflare_systemd] (secure file writing)
    def test_11_write_secure_file_refuses_a_sibling_directory_sharing_the_prefix(self):
        """A string-prefix test is not a directory boundary: "/x/keys_evil/f" starts with "/x/keys"."""
        with tempfile.TemporaryDirectory() as tmp:
            prefix = os.path.join(tmp, "keys")
            sibling = os.path.join(tmp, "keys_evil", "cloudflared-x.env")
            with self.assertRaises(ValueError):
                cf_systemd._write_secure_file(sibling, "TOKEN=secret\n", prefix)
            self.assertFalse(os.path.exists(os.path.dirname(sibling)))

    def test_12_write_secure_file_refuses_a_traversal_that_lands_in_the_sibling(self):
        # The realistic shape: a key that resolves out of the keys directory through "..".
        with tempfile.TemporaryDirectory() as tmp:
            prefix = os.path.join(tmp, "keys")
            traversal = os.path.join(prefix, "cloudflared-x", "..", "..", "keys_evil", "y.env")
            with self.assertRaises(ValueError):
                cf_systemd._write_secure_file(traversal, "TOKEN=secret\n", prefix)

    def test_13_write_secure_file_still_writes_inside_the_prefix(self):
        with tempfile.TemporaryDirectory() as tmp:
            prefix = os.path.join(tmp, "keys")
            path = os.path.join(prefix, "cloudflared-ok.env")
            cf_systemd._write_secure_file(path, "TOKEN=ok\n", prefix)
            with open(path) as f:
                self.assertEqual(f.read(), "TOKEN=ok\n")

    # Tests [@ANCHOR: cloudflare:ensure_unit_installed]
    def test_15_ensure_unit_installed_renders_once_and_skips_reload_when_unchanged(self):
        """start_tunnel_daemon's own test (test_04/test_05 above) mocks this function out
        entirely, so its real rendering/idempotency logic had no direct test at all. Real
        filesystem, matching test_09/test_10's own convention for this file -- only
        subprocess.run (`daemon-reload`) and the resolved binary path are mocked."""
        self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_systemd._resolve_cloudflared_bin",
            return_value="/usr/local/bin/cloudflared",
        )
        mock_run = self._mock_run(returncode=0, stdout="")
        with tempfile.TemporaryDirectory() as tmp_unit_dir:
            self.safe_patch(
                "odoo.addons.cloudflare.utils.cloudflare_systemd._USER_UNIT_DIR",
                new=tmp_unit_dir,
            )
            cf_systemd._ensure_unit_installed()
            unit_path = os.path.join(tmp_unit_dir, cf_systemd._UNIT_NAME_TEMPLATE)
            with open(unit_path) as f:
                rendered = f.read()
            self.assertIn("/usr/local/bin/cloudflared", rendered)
            self.assertNotIn("{cloudflared_bin}", rendered)
            mock_run.assert_called_once()
            self.assertEqual(mock_run.call_args[0][0][-1], "daemon-reload")
            no_leftovers = [
                f for f in os.listdir(tmp_unit_dir) if f.startswith(".cloudflared_unit_")
            ]
            self.assertEqual(no_leftovers, [])

            # Second call with identical rendered content: no daemon-reload, no rewrite.
            mock_run.reset_mock()
            before_mtime = os.stat(unit_path).st_mtime_ns
            cf_systemd._ensure_unit_installed()
            mock_run.assert_not_called()
            self.assertEqual(os.stat(unit_path).st_mtime_ns, before_mtime)

    # Tests [@ANCHOR: cloudflare:start_tunnel_daemon]
    def test_14_the_unit_never_puts_the_run_token_on_the_command_line(self):
        """An argument is readable by every local account through /proc/<pid>/cmdline and ps. The token
        must reach cloudflared through its TUNNEL_TOKEN environment variable (EnvironmentFile=) instead."""
        with open(cf_systemd._UNIT_TEMPLATE_PATH) as f:
            template = f.read()
        exec_lines = [l for l in template.splitlines() if l.startswith("ExecStart=")]
        self.assertEqual(len(exec_lines), 1)
        self.assertNotIn("--token", exec_lines[0])
        self.assertNotIn("${", exec_lines[0])
        self.assertNotIn("TOKEN", exec_lines[0])
        self.assertTrue(any(l.startswith("EnvironmentFile=") for l in template.splitlines()))
