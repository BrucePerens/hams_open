# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import datetime
import os
import tempfile
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import check_github_pat_expiry as check_mod


def _write_token(path, token="fake-github-pat-value"):
    with open(path, "w", encoding="utf-8") as f:
        f.write(f"{token}\n")


def _mock_response(expiration_header):
    resp = MagicMock()
    resp.headers.get.return_value = expiration_header
    return resp


@tagged("post_install", "-at_install")
class TestSeverityForDaysLeft(HamsTransactionCase):
    """The graduated-severity ladder itself, independent of the network/file-reading plumbing
    around it -- shared logic any future expiring-credential check could reuse."""

    def test_healthy_past_every_threshold_returns_none(self):
        self.assertIsNone(check_mod.severity_for_days_left(45))

    def test_thresholds_escalate_monotonically_as_days_left_decreases(self):
        self.assertEqual(check_mod.severity_for_days_left(30), "low")
        self.assertEqual(check_mod.severity_for_days_left(14), "medium")
        self.assertEqual(check_mod.severity_for_days_left(7), "high")
        self.assertEqual(check_mod.severity_for_days_left(2), "critical")

    def test_negative_days_left_already_expired_is_critical(self):
        self.assertEqual(check_mod.severity_for_days_left(-5), "critical")


@tagged("post_install", "-at_install")
class TestGithubPatExpiryCheck(HamsTransactionCase):
    """No live network call and no real credential -- urlopen is mocked, and the token file is a
    throwaway temp file, not the real /opt/hams/etc/keys/github_pat_ticket_triage.token."""

    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.token_path = os.path.join(self.tmpdir.name, "github_pat.token")
        self.orig_env = dict(os.environ)
        self.addCleanup(lambda: os.environ.clear() or os.environ.update(self.orig_env))
        os.environ["HAMS_GITHUB_PAT_PATH"] = self.token_path

    def _patch_urlopen(self, expiration_header):
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.check_github_pat_expiry.urllib.request.urlopen"
        )
        mock_urlopen.return_value.__enter__.return_value = _mock_response(expiration_header)
        return mock_urlopen

    def test_01_healthy_token_far_from_expiry_exits_zero(self):
        # Tests [@ANCHOR: pager_duty:github_pat_expiry_main]

        # Tests [@ANCHOR: pager_duty:read_github_pat]

        # Tests [@ANCHOR: pager_duty:fetch_github_pat_expiry]
        _write_token(self.token_path)
        future = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=300))
        self._patch_urlopen(future.strftime("%Y-%m-%d %H:%M:%S UTC"))
        self.assertEqual(check_mod.main(), 0)

    def test_02_token_close_to_expiry_exits_nonzero_with_critical_severity(self):
        _write_token(self.token_path)
        soon = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=1))
        self._patch_urlopen(soon.strftime("%Y-%m-%d %H:%M:%S UTC"))
        self.assertEqual(check_mod.main(), 1)

    def test_03_no_expiry_set_is_healthy_not_a_failure(self):
        """A fine-grained PAT can be created with no expiration date -- must not be treated as an
        error."""
        _write_token(self.token_path)
        self._patch_urlopen(None)
        self.assertEqual(check_mod.main(), 0)

    def test_04_missing_token_file_exits_nonzero_not_raises(self):
        """The real permission boundary this script depends on: if the file genuinely can't be
        read, fail cleanly -- never a raw traceback out of a monitoring check."""
        os.environ["HAMS_GITHUB_PAT_PATH"] = os.path.join(self.tmpdir.name, "does_not_exist.token")
        self.assertEqual(check_mod.main(), 1)

    def test_05_unparseable_expiration_header_exits_nonzero_not_raises(self):
        _write_token(self.token_path)
        self._patch_urlopen("not a real date at all")
        self.assertEqual(check_mod.main(), 1)

    def test_06_severity_prefix_is_printed_to_stderr_for_generalized_monitor_to_parse(self):
        """Real integration point with generalized_monitor.py's own execute_check(): the marker
        must land on stderr (where every synthetic script's diagnostic output already goes), not
        stdout, or the severity extraction there silently never matches."""
        import io
        import contextlib

        _write_token(self.token_path)
        soon = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=5))
        self._patch_urlopen(soon.strftime("%Y-%m-%d %H:%M:%S UTC"))

        captured_stderr = io.StringIO()
        with contextlib.redirect_stderr(captured_stderr):
            self.assertEqual(check_mod.main(), 1)
        self.assertIn("SEVERITY:high", captured_stderr.getvalue())
