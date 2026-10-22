# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests [@ANCHOR: pager_duty:check_freshness]: the "freshness" check type of generalized_monitor.py, which fails when the
newest record of a feed (or the last successful run of a timer-driven unit) is older than a limit, so a daemon that is
"active" but wedged is caught. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_freshness.py
"""
import os
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import redis as redis_lib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402

NOW = 1_800_000_000.0


class FakeRedis:
    def __init__(self, zsets=None, keys=None, error=None):
        self.zsets = zsets or {}
        self.keys = keys or {}
        self.error = error

    def zrevrange(self, name, start, stop, withscores=False):
        if self.error:
            raise self.error
        members = sorted(self.zsets.get(name, []), key=lambda m: m[1], reverse=True)
        return members[start:stop + 1]

    def get(self, name):
        if self.error:
            raise self.error
        return self.keys.get(name)


def run(target, max_age, redis=None, now=NOW):
    check = {"type": "freshness", "target": target, "max_age": max_age}
    return gm.check_freshness(check, now=now, redis_factory=(lambda: redis) if redis else None)


class RedisZsetTests(unittest.TestCase):
    KEY = "hams_prod:ham_dx_spots"

    def test_a_recent_newest_spot_passes_and_names_the_age(self):
        ok, msg = run(f"redis-zset:{self.KEY}", 1800, FakeRedis(zsets={self.KEY: [(b"a", NOW - 4000), (b"b", NOW - 20)]}))
        self.assertTrue(ok, msg)
        self.assertIn("20s old", msg)

    def test_the_newest_record_decides_not_the_oldest(self):
        ok, _ = run(f"redis-zset:{self.KEY}", 1800, FakeRedis(zsets={self.KEY: [(b"old", NOW - 9000), (b"new", NOW - 5)]}))
        self.assertTrue(ok)

    def test_a_feed_that_stopped_30_minutes_ago_fails_with_a_clear_message(self):
        ok, msg = run(f"redis-zset:{self.KEY}", 1800, FakeRedis(zsets={self.KEY: [(b"b", NOW - 1801)]}))
        self.assertFalse(ok)
        self.assertIn("30min", msg)
        self.assertIn(self.KEY, msg)
        self.assertIn("stopped", msg)

    def test_exactly_at_the_limit_still_passes(self):
        ok, _ = run(f"redis-zset:{self.KEY}", 1800, FakeRedis(zsets={self.KEY: [(b"b", NOW - 1800)]}))
        self.assertTrue(ok)

    def test_an_empty_or_missing_set_fails(self):
        ok, msg = run(f"redis-zset:{self.KEY}", 1800, FakeRedis())
        self.assertFalse(ok)
        self.assertIn("empty", msg)

    def test_a_score_slightly_in_the_future_is_fresh(self):
        ok, _ = run(f"redis-zset:{self.KEY}", 1800, FakeRedis(zsets={self.KEY: [(b"b", NOW + 30)]}))
        self.assertTrue(ok)

    def test_an_unreachable_redis_fails_with_the_reason(self):
        ok, msg = run(f"redis-zset:{self.KEY}", 1800, FakeRedis(error=redis_lib.ConnectionError("refused")))
        self.assertFalse(ok)
        self.assertIn("refused", msg)


class RedisKeyAndFileTests(unittest.TestCase):
    def test_epoch_and_iso_values_are_read(self):
        expected = 1_800_000_000.0
        for value in (1800000000, "1800000000", "1800000000.0", b"1800000000", "2027-01-15 08:00:00", "2027-01-15T08:00:00Z",
                      "2027-01-15T09:00:00+01:00"):
            self.assertEqual(gm.parse_freshness_time(value), expected, value)
        self.assertIsNone(gm.parse_freshness_time("soon"))

    def test_iso_time_age_is_computed_in_utc(self):
        iso = "2027-01-15 08:00:00"
        epoch = gm.parse_freshness_time(iso)
        ok, _ = run("redis-key:k", 600, FakeRedis(keys={"k": iso}), now=epoch + 300)
        self.assertTrue(ok)
        ok, _ = run("redis-key:k", 600, FakeRedis(keys={"k": iso}), now=epoch + 601)
        self.assertFalse(ok)

    def test_a_missing_key_or_garbage_fails(self):
        self.assertFalse(run("redis-key:k", 600, FakeRedis())[0])
        ok, msg = run("redis-key:k", 600, FakeRedis(keys={"k": "yesterday-ish"}))
        self.assertFalse(ok)
        self.assertIn("not a time", msg)

    def test_file_mtime(self):
        with tempfile.NamedTemporaryFile() as f:
            now = os.stat(f.name).st_mtime
            self.assertTrue(run(f"file:{f.name}", 60, now=now + 30)[0])
            self.assertFalse(run(f"file:{f.name}", 60, now=now + 61)[0])
        self.assertFalse(run("file:/nonexistent/heartbeat", 60)[0])


def show(**props):
    base = {"LoadState": "loaded", "Type": "oneshot", "ActiveState": "inactive", "Result": "success",
            "ExecMainStartTimestamp": "", "ExecMainExitTimestamp": ""}
    base.update(props)
    out = "".join(f"{k}={v}\n" for k, v in base.items())
    return mock.patch.object(gm.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, out, ""))


class UnitTests(unittest.TestCase):
    def setUp(self):
        patcher = mock.patch.object(gm, "ensure_executable", return_value=("/bin/systemctl", None))
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_a_recent_successful_run_passes(self):
        with show(ExecMainExitTimestamp="Tue 2026-10-20 05:17:00 UTC"):
            now = gm.parse_systemd_timestamp("Tue 2026-10-20 05:17:00 UTC") + 3600
            ok, msg = run("unit:fcc.uls.sync.service", 3 * 86400, now=now)
        self.assertTrue(ok, msg)

    def test_a_run_older_than_the_limit_fails(self):
        with show(ExecMainExitTimestamp="Tue 2026-10-20 05:17:00 UTC"):
            now = gm.parse_systemd_timestamp("Tue 2026-10-20 05:17:00 UTC") + 4 * 86400
            ok, msg = run("unit:fcc.uls.sync.service", 3 * 86400, now=now)
        self.assertFalse(ok)
        self.assertIn("4d", msg)

    def test_a_failed_last_run_fails(self):
        with show(Result="exit-code", ExecMainExitTimestamp="Tue 2026-10-20 05:17:00 UTC"):
            ok, msg = run("unit:fcc.uls.sync.service", 86400, now=gm.parse_systemd_timestamp("Tue 2026-10-20 06:00:00 UTC"))
        self.assertFalse(ok)
        self.assertIn("did not succeed", msg)

    def test_a_run_in_progress_passes_until_it_has_run_longer_than_the_limit(self):
        start = "Tue 2026-10-20 05:00:00 UTC"
        t0 = gm.parse_systemd_timestamp(start)
        with show(ActiveState="activating", ExecMainStartTimestamp=start):
            self.assertTrue(run("unit:x.service", 7200, now=t0 + 600)[0])
            ok, msg = run("unit:x.service", 7200, now=t0 + 8000)
        self.assertFalse(ok)
        self.assertIn("hung", msg)

    def test_a_unit_that_never_ran_is_fine_just_after_boot_and_not_long_after(self):
        with show():
            with mock.patch("builtins.open", mock.mock_open(read_data="3600.00 1.00\n")):
                self.assertTrue(run("unit:x.service", 86400)[0])
            with mock.patch("builtins.open", mock.mock_open(read_data="900000.00 1.00\n")):
                ok, msg = run("unit:x.service", 86400)
        self.assertFalse(ok)
        self.assertIn("since boot", msg)

    def test_a_long_running_daemon_unit_is_refused_with_advice(self):
        with show(Type="simple", ActiveState="active"):
            ok, msg = run("unit:ham.dx.daemon.service", 1800)
        self.assertFalse(ok)
        self.assertIn("redis-zset", msg)

    def test_an_unknown_unit_fails(self):
        with show(LoadState="not-found"):
            self.assertFalse(run("unit:nope.service", 60)[0])

    def test_several_units_report_every_stale_one_in_a_single_message(self):
        outputs = {
            "a.service": "LoadState=loaded\nType=oneshot\nActiveState=inactive\nResult=success\nExecMainExitTimestamp=Tue 2026-10-20 05:17:00 UTC\n",
            "b.service": "LoadState=loaded\nType=oneshot\nActiveState=inactive\nResult=success\nExecMainExitTimestamp=Fri 2026-10-09 05:17:00 UTC\n",
            "c.service": "LoadState=loaded\nType=oneshot\nActiveState=inactive\nResult=exit-code\nExecMainExitTimestamp=Tue 2026-10-20 05:17:00 UTC\n",
        }

        def fake(cmd, **kw):
            unit = cmd[2]
            return subprocess.CompletedProcess(cmd, 0, outputs[unit], "")

        now = gm.parse_systemd_timestamp("Tue 2026-10-20 06:00:00 UTC")
        with mock.patch.object(gm.subprocess, "run", side_effect=fake):
            ok, msg = run("unit:a.service, b.service,c.service", 3 * 86400, now=now)
            self.assertFalse(ok)
            self.assertIn("2 of 3", msg)
            self.assertIn("b.service", msg)
            self.assertIn("c.service", msg)
            self.assertNotIn("a.service ended", msg)
            ok, msg = run("unit:a.service,a.service", 3 * 86400, now=now)
        self.assertTrue(ok, msg)

    def test_the_stale_units_are_sent_with_the_incident(self):
        """Tests [@ANCHOR: pager_duty:check_units]."""
        outputs = {
            "a.service": "LoadState=loaded\nType=oneshot\nActiveState=inactive\nResult=success\nExecMainExitTimestamp=Tue 2026-10-20 05:17:00 UTC\n",
            "b.service": "LoadState=loaded\nType=oneshot\nActiveState=inactive\nResult=exit-code\nExecMainExitTimestamp=Tue 2026-10-20 05:17:00 UTC\n",
        }
        now = gm.parse_systemd_timestamp("Tue 2026-10-20 06:00:00 UTC")
        check = {"type": "freshness", "target": "unit:a.service,b.service", "max_age": 86400, "id": 11}
        with mock.patch.object(gm.subprocess, "run", side_effect=lambda cmd, **kw: subprocess.CompletedProcess(cmd, 0, outputs[cmd[2]], "")):
            gm.CHECK_UNITS.clear()
            self.assertFalse(gm.check_freshness(check, now=now)[0])
            self.assertEqual(gm.CHECK_UNITS[11], ["b.service"])
            check = dict(check, id=12, target="unit:b.service")
            self.assertFalse(gm.check_freshness(check, now=now)[0])
            self.assertEqual(gm.CHECK_UNITS[12], ["b.service"])
            gm.CHECK_UNITS.clear()
            self.assertTrue(gm.check_freshness(dict(check, id=13, target="unit:a.service"), now=now)[0])
            self.assertNotIn(13, gm.CHECK_UNITS)

    def test_systemd_timestamps(self):
        self.assertEqual(gm.parse_systemd_timestamp("Tue 2026-10-20 05:17:00 UTC"), 1792473420.0)
        self.assertIsNone(gm.parse_systemd_timestamp(""))
        self.assertIsNone(gm.parse_systemd_timestamp("n/a"))


class ConfigurationTests(unittest.TestCase):
    def test_a_bad_scheme_or_missing_limit_fails_loudly_instead_of_passing(self):
        self.assertFalse(run("ham_dx_spots", 60)[0])
        self.assertFalse(run("redis-zset:", 60)[0])
        ok, msg = run("redis-zset:k", 0, FakeRedis(zsets={"k": [(b"a", NOW)]}))
        self.assertFalse(ok)
        self.assertIn("maximum age", msg)

    def test_execute_check_dispatches_the_freshness_type(self):
        with mock.patch.object(gm, "check_freshness", return_value=(True, "OK")) as cf:
            self.assertEqual(gm.execute_check({"type": "freshness", "target": "file:/x", "max_age": 5}), (True, "OK"))
        cf.assert_called_once()

    def test_human_age(self):
        self.assertEqual(gm._human_age(45), "45s")
        self.assertEqual(gm._human_age(1800), "30min")
        self.assertEqual(gm._human_age(3 * 3600 + 120), "3h02")
        self.assertEqual(gm._human_age(5 * 86400), "5d")


if __name__ == "__main__":
    unittest.main()
