# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Daemon stop races and health-check exhaustion in `zero_sudo.daemon.utils`."""
import subprocess
from unittest.mock import MagicMock

from odoo.exceptions import UserError
from odoo.tests.common import tagged

from . import common


@tagged("post_install", "-at_install")
class TestDaemonStopAndPollGuards(common.HamsTransactionCase):
    def _process(self):
        process = MagicMock()
        process.pid = 4242
        process.poll.return_value = None
        return process

    def test_stop_treats_a_daemon_that_already_exited_as_stopped(self):
        # Tests [@ANCHOR: zero_sudo:stop_daemon_process]
        """The daemon can exit between poll() and the signal: that is the goal reached, not an error."""
        self.safe_patch("odoo.addons.zero_sudo.models.daemon_utils.os.getpgid", return_value=4242)
        killpg = self.safe_patch("odoo.addons.zero_sudo.models.daemon_utils.os.killpg", side_effect=ProcessLookupError)
        process = self._process()
        self.env["zero_sudo.daemon.utils"]._stop_daemon_process(process)
        killpg.assert_called_once()
        process.wait.assert_not_called()

    def test_stop_escalates_to_sigkill_when_sigterm_is_ignored(self):
        # Tests [@ANCHOR: zero_sudo:stop_daemon_process]
        self.safe_patch("odoo.addons.zero_sudo.models.daemon_utils.os.getpgid", return_value=4242)
        killpg = self.safe_patch("odoo.addons.zero_sudo.models.daemon_utils.os.killpg")
        process = self._process()
        process.wait.side_effect = subprocess.TimeoutExpired("d", 5)
        self.env["zero_sudo.daemon.utils"]._stop_daemon_process(process)
        self.assertEqual([c.args[1] for c in killpg.call_args_list], [15, 9])

    def test_stop_tolerates_exit_between_sigterm_timeout_and_sigkill(self):
        # Tests [@ANCHOR: zero_sudo:stop_daemon_process]
        self.safe_patch("odoo.addons.zero_sudo.models.daemon_utils.os.getpgid", return_value=4242)
        killpg = self.safe_patch(
            "odoo.addons.zero_sudo.models.daemon_utils.os.killpg", side_effect=[None, ProcessLookupError]
        )
        process = self._process()
        process.wait.side_effect = subprocess.TimeoutExpired("d", 5)
        with self.assertLogs("odoo.addons.zero_sudo.models.daemon_utils", level="INFO") as logs:
            self.env["zero_sudo.daemon.utils"]._stop_daemon_process(process)
        self.assertEqual(killpg.call_count, 2)
        self.assertTrue(any("already exited before SIGKILL" in m for m in logs.output))

    def test_health_check_that_never_passes_raises_a_user_error(self):
        # Tests [@ANCHOR: zero_sudo:_poll_health_check]
        """A daemon that never becomes healthy must fail loudly, not report success."""
        with self.assertRaisesRegex(UserError, "health check failed"):
            self.env["zero_sudo.daemon.utils"]._poll_health_check("http://odoo:1/health", timeout=0, interval=0.1)
