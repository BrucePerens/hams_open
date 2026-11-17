#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
import glob
import hashlib
import os
import sqlite3
import sys
import tempfile
import unittest

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
        with open(os.path.join(snap, "files", hashlib.sha1(data).hexdigest()), "wb") as f:  # noqa: S324
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
        import json
        make_snapshot(self.repo)
        path = os.path.join(self.tmp.name, "c.json")
        with open(path, "w") as f:
            json.dump(self.cfg, f)
        self.assertEqual(rd.main(["x", path]), 0)
        self.cfg["tables"]["res_users"] = 1000
        with open(path, "w") as f:
            json.dump(self.cfg, f)
        self.assertEqual(rd.main(["x", path]), 1)


if __name__ == "__main__":
    unittest.main()
