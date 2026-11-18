#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
import glob
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
import unittest.mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import restore_drill as rd  # noqa: E402


def make_snapshot(repo, name="20261117T000000Z", users=10, canary="K6BP-canary", flip=False, status="complete"):
    snap = os.path.join(repo, name)
    os.makedirs(os.path.join(snap, "files"))
    with open(os.path.join(snap, "state.json"), "w") as f:
        f.write('{"status": "%s"}' % status)
    con = sqlite3.connect(os.path.join(snap, "db.sqlite"))
    con.execute("create table res_users(id integer)")
    con.executemany("insert into res_users values (?)", [(i,) for i in range(users)])
    con.execute("create table canary(v text)")
    con.execute("insert into canary values (?)", (canary,))
    con.commit()
    con.close()
    for i in range(5):
        data = b"attachment %d" % i
        name = hashlib.sha1(data).hexdigest()  # burn-ignore-legacy-protocol-hash
        with open(os.path.join(snap, "files", name), "wb") as f:
            f.write(data if not (flip and i == 0) else b"attachmenX 0")
    return snap


# [@ANCHOR: backup_management:COMM_test_restore_drill]
class TestRestoreDrill(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:restore_drill_main]
    # Tests [@ANCHOR: backup_management:restore_drill_check_database]
    # Tests [@ANCHOR: backup_management:restore_drill_check_files]
    # Tests [@ANCHOR: backup_management:restore_drill_newest_snapshot]
    # Tests [@ANCHOR: backup_management:restore_drill_needs_bruce]
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = os.path.join(self.tmp.name, "repo")
        os.makedirs(self.repo)
        self.cfg = {"engine": "plaintext", "repo": self.repo, "tables": {"res_users": 10}, "min_ratio": 0.9,
                    "canary": {"table": "canary", "column": "v", "value": "K6BP-canary"},
                    "content_addressed": True, "sample_files": 5}

    def scratch_dirs(self):
        return glob.glob(os.path.join(tempfile.gettempdir(), "restore_drill_*"))

    def test_good_restore_passes_and_cleans_up(self):
        make_snapshot(self.repo)
        before = set(self.scratch_dirs())
        rd.run_drill(self.cfg, log=lambda m: None)
        self.assertEqual(set(self.scratch_dirs()), before)

    def test_empty_backup_fails(self):
        make_snapshot(self.repo, users=0)
        with self.assertRaisesRegex(rd.DrillError, "check row_count"):
            rd.run_drill(self.cfg, log=lambda m: None)

    def test_no_snapshot_at_all_fails(self):
        with self.assertRaisesRegex(rd.DrillError, "no complete snapshot"):
            rd.run_drill(self.cfg, log=lambda m: None)

    def test_wrong_row_count_fails(self):
        make_snapshot(self.repo, users=5)
        with self.assertRaisesRegex(rd.DrillError, "check row_count"):
            rd.run_drill(self.cfg, log=lambda m: None)

    def test_wrong_canary_fails(self):
        make_snapshot(self.repo, canary="other")
        with self.assertRaisesRegex(rd.DrillError, "check canary"):
            rd.run_drill(self.cfg, log=lambda m: None)

    def test_flipped_file_content_fails_and_cleans_up(self):
        make_snapshot(self.repo, flip=True)
        before = set(self.scratch_dirs())
        with self.assertRaisesRegex(rd.DrillError, "check files"):
            rd.run_drill(self.cfg, log=lambda m: None)
        self.assertEqual(set(self.scratch_dirs()), before)

    def test_partial_newest_snapshot_is_skipped(self):
        make_snapshot(self.repo, name="20261116T000000Z")
        make_snapshot(self.repo, name="20261117T000000Z", users=0, status="partial")
        rd.run_drill(self.cfg, log=lambda m: None)

    def test_key_engines_fail_loudly(self):
        for engine in ("kopia", "pgbackrest", "b2"):
            self.cfg["engine"] = engine
            with self.assertRaisesRegex(rd.DrillError, "needs Bruce: key step"):
                rd.run_drill(self.cfg)
        self.cfg.update(engine="plaintext", crypto_canary=True)
        with self.assertRaisesRegex(rd.DrillError, "needs Bruce: key step"):
            rd.run_drill(self.cfg)

    def test_main_exit_codes(self):
        make_snapshot(self.repo)
        path = os.path.join(self.tmp.name, "c.json")
        with open(path, "w") as f:
            json.dump(self.cfg, f)
        self.assertEqual(rd.main(["x", path]), 0)
        self.cfg["tables"]["res_users"] = 1000
        with open(path, "w") as f:
            json.dump(self.cfg, f)
        self.assertEqual(rd.main(["x", path]), 1)

    def test_worker_runs_script_with_no_arguments_using_config_beside_it(self):
        # The backup worker's cmd is [script_path] with no arguments: a missing arg must not mean usage-and-exit-2.
        make_snapshot(self.repo)
        with open(os.path.join(self.tmp.name, "restore_drill.json"), "w") as f:
            json.dump(self.cfg, f)
        orig = rd.DEFAULT_CONFIG
        rd.DEFAULT_CONFIG = os.path.join(self.tmp.name, "restore_drill.json")
        self.addCleanup(setattr, rd, "DEFAULT_CONFIG", orig)
        self.assertEqual(rd.main(["x"]), 0)
        self.assertEqual(os.path.dirname(orig), os.path.dirname(os.path.abspath(rd.__file__)))
        self.assertEqual(rd.main(["x", "a", "b"]), 2)

    def test_server_tools_dir_is_on_the_import_path(self):
        self.assertIn("/opt/hams/src/hams_open/hams_shared/tools", rd.SHARED_TOOLS_DIRS)


def _have_postgres():
    try:
        return subprocess.run(["psql", "-X", "-Atc", "select 1", "-d", "postgres"], capture_output=True,
                              timeout=20, check=False).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


@unittest.skipUnless(_have_postgres(), "needs a local postgres the test user can create databases in")
# [@ANCHOR: backup_management:COMM_test_restore_drill_postgres]
class TestRestoreDrillPostgres(unittest.TestCase):
    # Tests [@ANCHOR: backup_management:restore_drill_check_postgres]
    # Tests [@ANCHOR: backup_management:restore_drill_compare]
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = os.path.join(self.tmp.name, "repo")
        os.makedirs(self.repo)
        self.cfg = {"engine": "plaintext", "database": "postgres", "repo": self.repo, "tables": {"res_users": 10},
                    "canary": {"table": "canary", "column": "v", "value": "K6BP-canary"}, "sample_files": 5,
                    "content_addressed": True}

    def snapshot(self, users=10, canary="K6BP-canary", dump_bytes=None):
        snap = make_snapshot(self.repo)
        os.remove(os.path.join(snap, "db.sqlite"))
        dump = os.path.join(snap, "db.dump")
        if dump_bytes is not None:
            with open(dump, "wb") as f:
                f.write(dump_bytes)
            return
        name = "drill_src_" + os.urandom(4).hex()
        subprocess.run(["createdb", name], check=True)
        try:
            sql = ("create table res_users(id int); insert into res_users select generate_series(1, %d);"
                   "create table canary(v text); insert into canary values ('%s');" % (users, canary))
            subprocess.run(["psql", "-X", "-q", "-d", name, "-c", sql], check=True)
            subprocess.run(["pg_dump", "-Fc", "-f", dump, name], check=True)
        finally:
            subprocess.run(["dropdb", "--if-exists", name], check=True)

    def scratch_dbs(self):
        out = subprocess.run(["psql", "-X", "-At", "-d", "postgres", "-c",
                              "select datname from pg_database where datname like 'restore_drill_%'"],
                             capture_output=True, text=True, check=True).stdout
        return set(out.split())

    def test_good_dump_passes_and_scratch_database_is_dropped(self):
        self.snapshot()
        before = self.scratch_dbs()
        rd.run_drill(self.cfg, log=lambda m: None)
        self.assertEqual(self.scratch_dbs(), before)

    def test_wrong_row_count_fails_and_drops(self):
        self.snapshot(users=3)
        before = self.scratch_dbs()
        with self.assertRaisesRegex(rd.DrillError, "check row_count"):
            rd.run_drill(self.cfg, log=lambda m: None)
        self.assertEqual(self.scratch_dbs(), before)

    def test_wrong_canary_fails(self):
        self.snapshot(canary="other")
        with self.assertRaisesRegex(rd.DrillError, "check canary"):
            rd.run_drill(self.cfg, log=lambda m: None)

    def test_empty_or_garbage_dump_fails_and_drops(self):
        for data in (b"", b"not a dump"):
            self.snapshot(dump_bytes=data)
            before = self.scratch_dbs()
            with self.assertRaisesRegex(rd.DrillError, "check database"):
                rd.run_drill(self.cfg, log=lambda m: None)
            self.assertEqual(self.scratch_dbs(), before)
            shutil.rmtree(self.repo)
            os.makedirs(self.repo)

    def test_missing_postgres_tools_report_postgres_tools_failed_not_a_second_error(self):
        # Tests [@ANCHOR: backup_management:restore_drill_check_postgres]
        # The scratch-database drop in the finally also runs the missing tool: its OSError used to replace the DrillError.
        def no_tools(argv, **_kw):
            raise FileNotFoundError(argv[0])
        messages = []
        with unittest.mock.patch.object(rd.subprocess, "run", side_effect=no_tools):
            with self.assertRaisesRegex(rd.DrillError, "postgres tools failed"):
                rd.check_postgres("/nonexistent.dump", self.cfg, messages.append)
        self.assertTrue(any("was not dropped" in m for m in messages))


if __name__ == "__main__":
    unittest.main()
