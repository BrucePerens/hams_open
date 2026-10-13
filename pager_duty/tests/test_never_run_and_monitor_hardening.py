# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The "never run" check status, and the monitor changes that let it run unattended on a real host (2026-10-12)."""
import importlib.util
import io
import os
import socket
from contextlib import redirect_stdout
from unittest.mock import MagicMock

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger

import odoo.addons.pager_duty.daemon.generalized_monitor as generalized_monitor

MIGRATION_PATH = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "migrations", "1.1", "post-migration.py"
)


def _load_migration():
    # Load the real migration file, the same one Odoo's upgrade runs.
    spec = importlib.util.spec_from_file_location("pager_duty_post_migration_1_1", MIGRATION_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class _Stop(Exception):
    pass


class _InProcessClient:
    """Stands in for the daemon's JSON-2 OdooClient and calls the same models in this test's own transaction, so the
    daemon's real reporting path runs against the real Odoo code (and rolls back when the test ends)."""

    def __init__(self, env):
        self.env = env

    def execute(self, model, method, **kwargs):
        records = self.env[model]
        if "ids" in kwargs:
            records = records.browse(kwargs.pop("ids"))
        context = kwargs.pop("context", None)
        if context:
            records = records.with_context(**context)
        return getattr(records, method)(**kwargs)


@tagged("post_install", "-at_install")
class TestNeverRunStatus(HamsTransactionCase):
    def test_01_a_new_check_has_never_run(self):
        # Tests [@ANCHOR: pager_duty:never_run_migration]
        check = self.env["pager.check"].create({"name": "Never Run Default", "check_type": "system"})
        self.assertEqual(check.status, "never_run")
        self.assertFalse(check.last_run)
        self.assertEqual(check.failure_threshold, 3)

    def test_02_migration_marks_only_checks_that_never_ran(self):
        # Tests [@ANCHOR: pager_duty:never_run_migration]
        Check = self.env["pager.check"]
        never = Check.create({"name": "Mig Never", "check_type": "system"})
        ran = Check.create({"name": "Mig Ran", "check_type": "system", "grace_period": 0})
        tuned = Check.create({"name": "Mig Tuned", "check_type": "system", "grace_period": 45})
        self.env.flush_all()
        # The state a pre-1.1 database holds: every row "passing", whether or not a monitor ever ran it.
        self.env.cr.execute(  # audit-ignore-sql: test fixture, bound params
            "UPDATE pager_check SET status = 'passing', grace_period = 0 WHERE id IN %s", ((never.id, ran.id),)
        )
        self.env.cr.execute(  # audit-ignore-sql: test fixture, bound params
            "UPDATE pager_check SET status = 'passing', last_run = now() WHERE id = %s", (ran.id,)
        )
        self.env.cr.execute(  # audit-ignore-sql: test fixture, bound params
            "UPDATE pager_check SET status = 'passing' WHERE id = %s", (tuned.id,)
        )
        self.env.invalidate_all()
        _load_migration().migrate(self.env.cr, "1.0")
        self.env.invalidate_all()
        self.assertEqual(never.status, "never_run")
        self.assertEqual(never.grace_period, 300)
        self.assertEqual(ran.status, "passing", "a check a monitor really ran keeps its status")
        self.assertEqual(ran.grace_period, 0, "and the grace period an administrator gave it")
        self.assertEqual(tuned.status, "never_run")
        self.assertEqual(tuned.grace_period, 45, "an explicit grace period is never replaced")

    def test_03_board_counts_never_run_checks(self):
        # Tests [@ANCHOR: pager_board_stats]
        # The board counts the checks of the current website (and company); a check with no website is another scope.
        website = self.env["website"].get_current_website()
        self.env["pager.check"].create(
            {"name": "Board Never Run", "check_type": "system", "website_id": website.id if website else False}
        )
        self.env.flush_all()
        data = self.env["pager.incident"].get_board_data()
        self.assertGreaterEqual(data["stats"]["never_run"], 1)

    def test_04_export_carries_a_threshold_only_when_it_differs_from_one(self):
        # Tests [@ANCHOR: generalized_pager_config]
        import json
        import tempfile

        Check = self.env["pager.check"]
        Check.create({"name": "Thr Default", "check_type": "system", "target": "cpu"})
        Check.create({"name": "Thr One", "check_type": "system", "target": "cpu", "failure_threshold": 1})
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "pager_config.json")
            self.safe_patch_object(type(Check), "_get_config_path", return_value=path)
            Check.action_push_to_json()
            with open(path, encoding="utf-8") as f:
                exported = {c["name"]: c for c in json.load(f)["checks"]}
        self.assertEqual(exported["Thr Default"]["threshold"], 3)
        self.assertNotIn("threshold", exported["Thr One"])


@tagged("post_install", "-at_install")
class TestSyntheticScriptsRunDirectly(HamsTransactionCase):
    def test_20_the_shipped_check_scripts_are_executable_with_a_shebang(self):
        # Tests [@ANCHOR: pager_duty:daemon_execute_check]
        # A "Synthetic Journey (Script)" check runs its script path directly (shlex.split, no interpreter), so the file must be
        # executable and name its interpreter. On hams1 (2026-10-12) both shipped scripts failed with "Permission denied".
        daemon_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "daemon")
        scripts = sorted(name for name in os.listdir(daemon_dir) if name.startswith("check_") and name.endswith(".py"))
        self.assertGreaterEqual(len(scripts), 2)
        for name in scripts:
            path = os.path.join(daemon_dir, name)
            with self.subTest(script=name):
                self.assertTrue(os.access(path, os.X_OK), f"{name} must be executable")
                with open(path, encoding="utf-8") as f:
                    self.assertEqual(f.readline().rstrip("\n"), "#!/usr/bin/env python3")


@tagged("post_install", "-at_install")
class TestMonitorHardening(HamsTransactionCase):
    @mute_logger("generalized_monitor")
    def test_10_odoo_client_accepts_the_key_registry_names(self):
        # Tests [@ANCHOR: pager_duty:get_odoo_client]
        # daemon_key_manager writes ODOO_RPC_LOGIN / ODOO_RPC_KEY into the daemon's key file.
        orig = dict(os.environ)
        try:
            for name in ("ODOO_USER", "ODOO_PASSWORD", "ODOO_RPC_LOGIN", "ODOO_RPC_KEY"):
                os.environ.pop(name, None)
            os.environ.update({"ODOO_RPC_LOGIN": "svc_login", "ODOO_RPC_KEY": "svc_key_value", "ODOO_DB": "d"})
            client = generalized_monitor.get_odoo_client(generalized_monitor.logger, {})
            self.assertIsNotNone(client)
            self.assertEqual(client.headers["Authorization"], "bearer svc_key_value")
            # The daemon's own names win when both are set.
            os.environ.update({"ODOO_PASSWORD": "own_pass"})
            client = generalized_monitor.get_odoo_client(generalized_monitor.logger, {})
            self.assertEqual(client.headers["Authorization"], "bearer own_pass")
            # Neither: refuses to start.
            os.environ.pop("ODOO_PASSWORD")
            os.environ.pop("ODOO_RPC_KEY")
            self.assertIsNone(generalized_monitor.get_odoo_client(generalized_monitor.logger, {}))
        finally:
            os.environ.clear()
            os.environ.update(orig)

    def test_11_redis_check_logs_in_as_the_service_when_it_has_no_credentials(self):
        # Tests [@ANCHOR: pager_duty:daemon_execute_check]
        redis_cls = self.safe_patch("redis.Redis")
        redis_cls.return_value.ping.return_value = True
        self.safe_patch_object(generalized_monitor, "REDIS_USERNAME", "acl_user")
        self.safe_patch_object(generalized_monitor, "REDIS_PASS", "acl_pass")
        ok, _msg = generalized_monitor.execute_check({"type": "redis", "target": "redis"})
        self.assertTrue(ok)
        kwargs = redis_cls.call_args.kwargs
        self.assertEqual(kwargs["username"], "acl_user")
        self.assertEqual(kwargs["password"], "acl_pass")
        # A password on the check itself is used as given, and the service login is not mixed in.
        ok, _msg = generalized_monitor.execute_check(
            {"type": "redis", "target": "redis", "password": "own", "user": "own_user"}
        )
        kwargs = redis_cls.call_args.kwargs
        self.assertEqual((kwargs["username"], kwargs["password"]), ("own_user", "own"))

    def test_12_postgres_check_uses_the_environment_login_when_it_has_no_password(self):
        # Tests [@ANCHOR: pager_duty:daemon_execute_check]
        pg = self.safe_patch("odoo.addons.pager_duty.daemon.generalized_monitor.psycopg2")
        pg.connect.return_value.cursor.return_value.__enter__.return_value.fetchone.return_value = [1]
        orig = dict(os.environ)
        try:
            os.environ.update({"DB_USER": "pager_monitor_ro", "DB_PASS": "p", "DB_NAME": "hams_prod"})
            ok, _msg = generalized_monitor.execute_check(
                {"type": "postgres", "target": "postgres", "user": "postgres", "dbname": "postgres"}
            )
            self.assertTrue(ok)
            kwargs = pg.connect.call_args.kwargs
            self.assertEqual((kwargs["user"], kwargs["password"], kwargs["dbname"]), ("pager_monitor_ro", "p", "hams_prod"))
            # A check that carries its own password keeps its own login.
            generalized_monitor.execute_check(
                {"type": "postgres", "target": "postgres", "user": "mine", "password": "x", "dbname": "other"}
            )
            kwargs = pg.connect.call_args.kwargs
            self.assertEqual((kwargs["user"], kwargs["password"], kwargs["dbname"]), ("mine", "x", "other"))
        finally:
            os.environ.clear()
            os.environ.update(orig)

    def test_13_an_inactive_oneshot_whose_last_run_succeeded_is_healthy(self):
        # Tests [@ANCHOR: pager_duty:oneshot_last_run_succeeded]
        def fake_run(cmd, **_kw):
            res = MagicMock()
            res.returncode = 0
            if "is-active" in cmd:
                res.returncode, res.stdout = 3, "inactive\n"
            elif cmd[1] == "show":
                res.stdout = f"Type=oneshot\nResult={self.show_result}\nLoadState=loaded\n"
            return res

        self.safe_patch("odoo.addons.pager_duty.daemon.generalized_monitor.subprocess.run", side_effect=fake_run)
        self.safe_patch_object(generalized_monitor, "ensure_executable", return_value=("/bin/systemctl", ""))
        check = {"type": "systemd", "target": "sync.service"}
        self.show_result = "success"
        ok, msg = generalized_monitor.execute_check(check)
        self.assertTrue(ok, msg)
        self.show_result = "exit-code"
        ok, msg = generalized_monitor.execute_check(check)
        self.assertFalse(ok)
        self.assertIn("sync.service (inactive)", msg)

    def test_14_a_failure_is_reported_only_after_the_threshold_of_consecutive_failures(self):
        # Tests [@ANCHOR: pager_duty:polling_thread]
        gm = generalized_monitor
        self.addCleanup(gm.FAILING_CHECKS.clear)
        self.addCleanup(gm.FAILING_NAMES.clear)
        results = iter([(False, "boom"), (False, "boom"), (True, "OK"), (False, "boom"), (False, "boom"), (False, "boom")])
        self.safe_patch_object(gm, "execute_check", side_effect=lambda *_a, **_k: next(results))
        reported = self.safe_patch_object(gm, "report")
        self.safe_patch_object(gm, "is_in_maintenance", return_value=False)
        sleeps = []

        def fake_sleep(seconds):
            sleeps.append(seconds)
            if len(sleeps) >= 6:
                raise _Stop

        self.safe_patch_object(gm.time, "sleep", side_effect=fake_sleep)
        client = MagicMock()
        check = {"id": 7001, "name": "Threshold Check", "type": "system", "interval": 300, "threshold": 3}
        with self.assertRaises(_Stop):
            gm.polling_thread(client, check)
        # Runs: F F P F F F. The first two failures and the pass are not reported; the third failure in a row is.
        self.assertEqual(reported.call_count, 1)
        self.assertIn("Threshold Check", reported.call_args.args)
        # A failure is retried within seconds, not after the 300 second interval.
        self.assertIn(gm.RETRY_INTERVAL_SECONDS, sleeps)
        # Every run, including the first, is recorded in Odoo (status and last_run).
        writes = [c.kwargs["vals"]["status"] for c in client.execute.call_args_list if c.args[1] == "write"]
        self.assertEqual(writes[:3], ["failing", "failing", "passing"])
        self.assertTrue(all("last_run" in c.kwargs["vals"] for c in client.execute.call_args_list if c.args[1] == "write"))

    def test_14b_failures_during_pagerduty_maintenance_are_not_counted_or_reported(self):
        # Tests [@ANCHOR: pager_duty:polling_thread]
        gm = generalized_monitor
        self.addCleanup(gm.FAILING_CHECKS.clear)
        self.addCleanup(gm.FAILING_NAMES.clear)
        results = iter([(False, "boom")] * 5 + [(False, "boom")] * 3)
        self.safe_patch_object(gm, "execute_check", side_effect=lambda *_a, **_k: next(results))
        reported = self.safe_patch_object(gm, "report")
        self.safe_patch_object(gm, "is_in_maintenance", return_value=False)
        state = {"flag": gm.pagerduty_maintenance.ACTIVE}
        self.safe_patch_object(
            gm.pagerduty_maintenance, "read_flag", side_effect=lambda: {"state": state["flag"], "error": ""}
        )
        self.safe_patch_object(gm.pagerduty_maintenance, "describe", return_value="test maintenance")
        sleeps = []

        def fake_sleep(seconds):
            sleeps.append(seconds)
            if len(sleeps) == 5:  # the maintenance window ends
                state["flag"] = gm.pagerduty_maintenance.ABSENT
            if len(sleeps) >= 8:
                raise _Stop

        self.safe_patch_object(gm.time, "sleep", side_effect=fake_sleep)
        check = {"id": 7003, "name": "Maintenance Check", "type": "system", "interval": 60, "threshold": 3}
        try:
            gm.polling_thread(MagicMock(), check)
        except _Stop:
            pass
        # Five failing runs inside maintenance: none counted, none reported. After it ends the normal three consecutive
        # failures are needed again, and the third one reports.
        self.assertEqual(reported.call_count, 1)

    def test_15_the_first_run_is_recorded_so_a_daily_check_is_not_never_run_for_a_day(self):
        # Tests [@ANCHOR: pager_duty:record_check_status]
        gm = generalized_monitor
        self.safe_patch_object(gm, "execute_check", return_value=(True, "OK"))
        slept = []

        def fake_sleep(seconds):
            slept.append(seconds)
            raise _Stop

        self.safe_patch_object(gm.time, "sleep", side_effect=fake_sleep)
        client = MagicMock()
        with self.assertRaises(_Stop):
            gm.polling_thread(client, {"id": 7002, "name": "Daily", "type": "system", "interval": 86400})
        self.assertEqual(client.execute.call_args.kwargs["vals"]["status"], "passing")
        self.assertLessEqual(slept[0], gm.STARTUP_JITTER_CAP_SECONDS, "startup jitter is capped, not up to a day")

    def test_16_check_once_prints_every_result_and_reports_nothing(self):
        # Tests [@ANCHOR: pager_duty:check_once]
        gm = generalized_monitor
        results = {"A": (True, "OK"), "B": (False, "[SEVERITY:low] broke")}
        self.safe_patch_object(gm, "execute_check", side_effect=lambda check, _c=None: results[check["name"]])
        reported = self.safe_patch_object(gm, "report")
        out = io.StringIO()
        with redirect_stdout(out):
            status = gm.check_once([{"name": "A"}, {"name": "B"}, {"name": "L", "type": "log"}])
        self.assertEqual(status, 1)
        text = out.getvalue()
        self.assertIn("PASS A: OK", text)
        self.assertIn("FAIL B: broke", text)
        self.assertIn("SKIP L", text)
        reported.assert_not_called()
        with redirect_stdout(io.StringIO()):
            self.assertEqual(gm.check_once([{"name": "A"}]), 0)

    def test_17_an_injected_failure_creates_an_incident_and_recovery_resolves_it(self):
        # Tests [@ANCHOR: pager_duty:polling_thread]
        # A deliberate, harmless failure: a TCP check against a loopback port nothing listens on. The daemon's real polling
        # loop runs against the real models (in-process client): the failure is confirmed on the second run, reported as an
        # incident with the check's name as source and written back to the row; then something starts listening, and three
        # passing runs resolve the incident and leave the row passing.
        gm = generalized_monitor
        self.addCleanup(gm.FAILING_CHECKS.clear)
        self.addCleanup(gm.FAILING_NAMES.clear)
        probe = socket.socket()
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
        probe.close()
        row = self.env["pager.check"].create(
            {"name": "Injected Failure Check", "check_type": "tcp", "target": "127.0.0.1", "port": port, "failure_threshold": 2}
        )
        self.assertEqual(row.status, "never_run")
        check = {"id": row.id, "name": row.name, "type": "tcp", "target": "127.0.0.1", "port": port, "interval": 60, "threshold": 2}
        listeners = []
        sleeps = []

        def fake_sleep(seconds):
            sleeps.append(seconds)
            if len(sleeps) == 2:  # the failure has been reported; bring the service "back"
                server = socket.create_server(("127.0.0.1", port))
                self.addCleanup(server.close)
                listeners.append(server)
            if len(sleeps) >= 5:
                raise _Stop

        self.safe_patch_object(gm.time, "sleep", side_effect=fake_sleep)
        self.safe_patch_object(gm, "is_in_maintenance", return_value=False)
        self.safe_patch("odoo.addons.pager_duty.models.incident.redis_pool", None)  # no shared rate-limit state
        seen = {}
        original = gm.report

        def spying_report(*args, **kwargs):
            original(*args, **kwargs)
            self.env.flush_all()
            self.env.cr.execute(  # audit-ignore-sql: test read, bound params
                "SELECT severity, status FROM pager_incident WHERE source = %s", (row.name,)
            )
            seen["rows"] = self.env.cr.fetchall()

        self.safe_patch_object(gm, "report", side_effect=spying_report)
        # Not assertRaises: Odoo's assertRaises rolls the transaction back to a savepoint when the exception arrives, which
        # would undo the very work (the incident, the status writes) this test checks.
        try:
            gm.polling_thread(_InProcessClient(self.env), check)
        except _Stop:
            pass
        else:
            self.fail("the polling loop never ends by itself; the patched sleep must stop it")
        self.assertEqual(seen["rows"], [("high", "open")], "the confirmed failure created one open, paging incident")
        self.env.flush_all()
        self.env.cr.execute(  # audit-ignore-sql: test read, bound params
            "SELECT status FROM pager_incident WHERE source = %s", (row.name,)
        )
        self.assertEqual(self.env.cr.fetchall(), [("resolved",)], "one incident, resolved by three passing runs")
        row.invalidate_recordset(["status", "last_run"])
        self.assertEqual(row.status, "passing")
        self.assertTrue(row.last_run)
