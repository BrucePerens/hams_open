# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import contextlib
import io
import os
import tempfile
import time
from unittest import mock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import check_recordings_disk_usage as check_mod

DAY = 86400.0
GB = 1000 ** 3


@tagged("post_install", "-at_install")
class TestRecordingsDiskSeverity(HamsTransactionCase):
    # Tests [@ANCHOR: pager_duty:recordings_disk_severity]
    def test_healthy_below_seventy_percent(self):
        self.assertIsNone(check_mod.severity_for_percent(0))
        self.assertIsNone(check_mod.severity_for_percent(69.9))

    def test_warns_from_seventy_percent_without_paging(self):
        # `medium` is recorded as a trend-tracked incident and does not page on-call immediately.
        self.assertEqual(check_mod.severity_for_percent(70), "medium")
        self.assertEqual(check_mod.severity_for_percent(84.9), "medium")

    def test_fails_and_pages_from_eighty_five_percent(self):
        self.assertEqual(check_mod.severity_for_percent(85), "high")
        self.assertEqual(check_mod.severity_for_percent(100), "high")
        self.assertNotIn(check_mod.severity_for_percent(85), ("low", "medium"))

    def test_thresholds_are_parameters(self):
        self.assertEqual(check_mod.severity_for_percent(50, warn=40, fail=60), "medium")
        self.assertEqual(check_mod.severity_for_percent(60, warn=40, fail=60), "high")


@tagged("post_install", "-at_install")
class TestRecordingsDiskUsage(HamsTransactionCase):
    # Tests [@ANCHOR: pager_duty:recordings_disk_usage]
    def test_percent_is_df_use_percent_of_the_filesystem(self):
        # 100 blocks, 40 free to root, 30 available to everyone: used 60, df says 60 / (60 + 30) = 66.7 percent.
        stat = mock.Mock(f_blocks=100, f_bfree=40, f_bavail=30, f_frsize=1000)
        self.safe_patch_object(check_mod.os, "statvfs", return_value=stat)
        percent, used, available = check_mod.filesystem_usage("/")
        self.assertAlmostEqual(percent, 66.667, places=2)
        self.assertEqual((used, available), (60000, 30000))

    def test_a_missing_directory_is_measured_through_its_nearest_parent(self):
        with tempfile.TemporaryDirectory() as tmp:
            missing = os.path.join(tmp, "not", "yet")
            self.assertEqual(check_mod.nearest_existing_directory(missing), os.path.abspath(tmp))
            percent, _used, _available = check_mod.filesystem_usage(missing)
        self.assertTrue(0 <= percent <= 100)


@tagged("post_install", "-at_install")
class TestRecordingsVolumeEstimate(HamsTransactionCase):
    # Tests [@ANCHOR: pager_duty:recordings_volume]
    # Tests [@ANCHOR: pager_duty:recordings_estimate]
    def _touch(self, path, size, age_days):
        with open(path, "wb") as f:  # audit-ignore-path
            f.write(b"\0" * size)
        stamp = time.time() - age_days * DAY
        os.utime(path, (stamp, stamp))

    def test_counts_files_and_recent_bytes_in_the_directory_and_sessions(self):
        with tempfile.TemporaryDirectory() as tmp:
            os.makedirs(os.path.join(tmp, "sessions"))
            os.makedirs(os.path.join(tmp, "elsewhere"))
            self._touch(os.path.join(tmp, "a.opus"), 1000, 1)
            self._touch(os.path.join(tmp, "a.json"), 100, 1)
            self._touch(os.path.join(tmp, "old.opus"), 5000, 30)
            self._touch(os.path.join(tmp, "sessions", "s.json"), 10, 2)
            self._touch(os.path.join(tmp, "elsewhere", "ignored.bin"), 99999, 0)
            count, total, recent, complete, oldest = check_mod.directory_volume(tmp)
        self.assertEqual((count, total, recent, complete), (4, 6110, 1110, True))
        self.assertAlmostEqual(oldest, time.time() - 30 * DAY, delta=60)

    def test_a_directory_that_does_not_exist_has_no_files(self):
        self.assertEqual(check_mod.directory_volume("/nonexistent/qso_recordings"), (0, 0, 0, True, None))

    def test_the_listing_stops_at_its_time_budget_and_says_so(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._touch(os.path.join(tmp, "a.opus"), 10, 0)
            _c, _t, _r, complete, _o = check_mod.directory_volume(tmp, budget=-1)
        self.assertFalse(complete)

    def test_days_to_fail_from_the_growth_rate(self):
        # 70 GB written in the last 7 days is 10 GB/day. Used 600 GB, available 400 GB: 85 percent is 850 GB,
        # 250 GB of room, 25 days.
        volume = (100, 80 * GB, 70 * GB, True, time.time() - 30 * DAY)
        rate, days = check_mod.estimate(600 * GB, 400 * GB, volume)
        self.assertAlmostEqual(rate, 10 * GB)
        self.assertAlmostEqual(days, 25.0)

    def test_no_growth_has_no_estimate_and_a_crossed_line_is_zero_days(self):
        self.assertEqual(check_mod.estimate(100 * GB, 900 * GB, (1, 1, 0, True, None))[1], None)
        self.assertEqual(check_mod.estimate(900 * GB, 100 * GB, (1, 1, GB, True, time.time() - DAY))[1], 0.0)

    def test_a_directory_younger_than_the_window_is_averaged_over_its_own_age(self):
        # 6 GB written over 12 hours is 12 GB/day, not 6 GB / 7 days.
        now = 1_000_000_000.0
        rate, _days = check_mod.estimate(100 * GB, 900 * GB, (10, 6 * GB, 6 * GB, True, now - 0.5 * DAY), now=now)
        self.assertAlmostEqual(rate, 12 * GB)
        # A directory seconds old is not divided by seconds: the span has a one hour floor.
        rate, _days = check_mod.estimate(100 * GB, 900 * GB, (1, GB, GB, True, now - 5), now=now)
        self.assertAlmostEqual(rate, 24 * GB)

    def test_a_huge_horizon_reads_as_more_than_ten_years(self):
        line = check_mod.summary_line("/r", 24.0, (10, 2 * GB, 1000, True, None), 1000, 400000.0)
        self.assertIn("more than 10 years to 85%", line)

    def test_summary_line_carries_files_size_rate_and_days(self):
        line = check_mod.summary_line("/r", 24.0, (10, 2 * GB, 7 * GB, True, None), 1 * GB, 55.0)
        for part in ("/r", "10 files", "2.0 GB", "1.0 GB/day", "24% used", "55 days to 85%"):
            self.assertIn(part, line)


@tagged("post_install", "-at_install")
class TestRecordingsDiskCheck(HamsTransactionCase):
    # Tests [@ANCHOR: pager_duty:recordings_disk_main]
    def _run(self, percent, argv=()):
        out, err = io.StringIO(), io.StringIO()
        self.safe_patch_object(check_mod, "filesystem_usage", return_value=(percent, 600 * GB, 400 * GB))
        with tempfile.TemporaryDirectory() as tmp:
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = check_mod.main([tmp, *argv])
        return code, out.getvalue(), err.getvalue()

    def test_healthy_prints_the_estimate_line_and_exits_zero(self):
        code, out, err = self._run(24)
        self.assertEqual(code, 0)
        self.assertIn("recordings in ", out)
        self.assertIn("24% used", out)
        self.assertEqual(err, "")

    def test_warning_exits_one_with_a_medium_severity_marker_and_the_essentials_first(self):
        code, _out, err = self._run(72)
        self.assertEqual(code, 1)
        self.assertEqual(err.splitlines()[0], "SEVERITY:medium")
        self.assertIn("72%", err.splitlines()[1])
        self.assertIn("warning", err.splitlines()[1])
        self.assertLessEqual(len(err.splitlines()[1]), 80)

    def test_failure_exits_one_with_a_high_severity_marker(self):
        code, _out, err = self._run(86)
        self.assertEqual(code, 1)
        self.assertEqual(err.splitlines()[0], "SEVERITY:high")
        self.assertIn("FAILING", err.splitlines()[1])

    def test_the_monitor_keeps_the_marker_and_the_percent_in_its_first_hundred_characters(self):
        _code, _out, err = self._run(86)
        self.assertIn("86%", err[:100])
        self.assertIn("SEVERITY:high", err[:100])

    def test_thresholds_can_be_overridden_on_the_command_line(self):
        code, _out, err = self._run(50, ["--warn", "40", "--fail", "60"])
        self.assertEqual(code, 1)
        self.assertEqual(err.splitlines()[0], "SEVERITY:medium")

    def test_an_unmeasurable_filesystem_is_high(self):
        err = io.StringIO()
        self.safe_patch_object(check_mod, "filesystem_usage", side_effect=OSError("boom"))
        with contextlib.redirect_stderr(err):
            code = check_mod.main(["/x"])
        self.assertEqual(code, 1)
        self.assertEqual(err.getvalue().splitlines()[0], "SEVERITY:high")
