#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Content-checking restore drill: usage `restore_drill.py CONFIG.json`.

Exits 0 only when the newest complete snapshot restores into a scratch directory and passes every content check
(row counts, a canary value, sampled file hashes). An empty or wrong restore cannot exit 0. Every failing check is named
on stderr (never a key, passphrase or secret value). The scratch directory is removed on every exit path.

Keyless part only: engine "plaintext" reads a directory repository laid out as
  <repo>/<snapshot>/state.json   {"status": "complete"|"partial"}
  <repo>/<snapshot>/db.sqlite    database copy
  <repo>/<snapshot>/files/...    file tree (content-addressed = named by SHA-1 of content)
Engines needing a repository key or passphrase (kopia, pgbackrest, B2) are stubs that fail with NEEDS_BRUCE.
Reuses b2_backup.restore_sample / sha1_file / sha256_file for the file check.
"""
import json
import os
import random  # audit-ignore-weak-random: fixed seed picks which files to sample; not a security property
import shutil
import sqlite3
import sys
import tempfile
import types

sys.path.insert(0, os.environ.get("HAMS_SHARED_TOOLS") or os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "hams_shared", "tools"))
import b2_backup  # noqa: E402

NEEDS_BRUCE = "needs Bruce: key step"


# [@ANCHOR: backup_management:restore_drill_error]
class DrillError(RuntimeError):
    pass


# [@ANCHOR: backup_management:restore_drill_needs_bruce]
def needs_bruce(step):
    """A step that needs a repository key, passphrase or B2 key. Fails loudly; never silently skips."""
    raise DrillError(f"{NEEDS_BRUCE}: {step}")


# [@ANCHOR: backup_management:restore_drill_local_runner]
class LocalRunner:
    """Stands in for kopia over a plaintext directory repository: handles only `ls -r ROOT` and `restore SRC DEST`."""

    def run(self, argv, env=None, capture=False, **_kw):
        verb = argv[1]
        if verb == "ls":
            root = argv[3]
            out = [os.path.join(d, f) for d, _s, fs in os.walk(root) for f in fs]
            return types.SimpleNamespace(stdout="\n".join(sorted(out)), returncode=0)
        if verb == "restore":
            shutil.copy2(argv[2], argv[3])
            return types.SimpleNamespace(stdout="", returncode=0)
        raise DrillError(f"unsupported runner verb {verb}")


# [@ANCHOR: backup_management:restore_drill_newest_snapshot]
def newest_complete_snapshot(repo):
    """Newest (by name, names are sortable stamps) snapshot whose state.json says complete; partial ones are skipped."""
    for name in sorted(os.listdir(repo), reverse=True):
        try:
            with open(os.path.join(repo, name, "state.json")) as f:
                if json.load(f).get("status") == "complete":
                    return os.path.join(repo, name)
        except (OSError, ValueError):
            continue
    raise DrillError("check snapshot: no complete snapshot in the repository")


# [@ANCHOR: backup_management:restore_drill_check_database]
def check_database(db_path, cfg, log):
    """Row counts of the fixed tables against the expected counts (within min_ratio), then the canary value."""
    try:
        con = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
        try:
            for table, expected in cfg["tables"].items():
                if not table.isidentifier():
                    raise DrillError(f"check row_count: bad table name {table!r}")
                try:
                    n = con.execute(f"select count(*) from {table}").fetchone()[0]
                except sqlite3.Error:
                    raise DrillError(f"check row_count: table {table} missing or unreadable") from None
                log(f"rows {table}={n} expected={expected}")
                if n == 0 or n < expected * cfg.get("min_ratio", 0.9):
                    raise DrillError(f"check row_count: {table} has {n} rows, expected about {expected}")
            canary = cfg["canary"]
            if not (canary["table"].isidentifier() and canary["column"].isidentifier()):
                raise DrillError("check canary: bad table or column name")
            row = con.execute(f"select {canary['column']} from {canary['table']} limit 1").fetchone()
            if row is None or str(row[0]) != canary["value"]:
                raise DrillError("check canary: the canary row is missing or has the wrong value")
            log("canary ok")
        finally:
            con.close()
    except sqlite3.Error as exc:
        raise DrillError(f"check database: not a readable database ({type(exc).__name__})") from None


# [@ANCHOR: backup_management:restore_drill_check_files]
def check_files(snap_dir, scratch, cfg, log):
    """Samples files with b2_backup.restore_sample (SHA-1 name for a content-addressed tree, else SHA-256 vs live)."""
    files_dir = os.path.join(snap_dir, "files")
    source = types.SimpleNamespace(name="files", content_addressed=bool(cfg.get("content_addressed")),
                                   path=cfg.get("live_files_dir", ""))
    fake_cfg = types.SimpleNamespace(kopia="kopia", restore_sample_files=int(cfg.get("sample_files", 20)))
    snap = {"rootEntry": {"obj": files_dir}}
    try:
        result = b2_backup.restore_sample(fake_cfg, {}, source, snap, random.Random(0), LocalRunner(), scratch)
    except b2_backup.BackupError as exc:
        raise DrillError(f"check files: {exc}") from None
    log(f"files sampled={result['sampled']} verified={result['verified']} changed_since={result['changed_since']}")


# [@ANCHOR: backup_management:restore_drill_main]
def run_drill(cfg, log=print):
    """Runs every check against the newest complete snapshot. Raises DrillError on the first failure."""
    engine = cfg.get("engine")
    if engine != "plaintext":
        needs_bruce(f"engine {engine!r} restore needs the repository key or passphrase")
    if cfg.get("crypto_canary"):
        needs_bruce("encrypted-field canary needs the Odoo crypto key")
    snap = newest_complete_snapshot(cfg["repo"])
    scratch = tempfile.mkdtemp(prefix="restore_drill_")  # mode 0700
    try:
        db_copy = os.path.join(scratch, "db.sqlite")
        try:
            shutil.copy2(os.path.join(snap, "db.sqlite"), db_copy)
        except OSError:
            raise DrillError("check database: the snapshot has no database") from None
        check_database(db_copy, cfg, log)
        check_files(snap, scratch, cfg, log)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def main(argv):
    if len(argv) != 2:
        print("usage: restore_drill.py CONFIG.json", file=sys.stderr)
        return 2
    try:
        with open(argv[1]) as f:
            cfg = json.load(f)
        run_drill(cfg, log=lambda m: print(m, flush=True))
    except DrillError as exc:
        print(f"DRILL FAILED: {exc}", file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError) as exc:
        print(f"DRILL FAILED: bad configuration or repository ({type(exc).__name__})", file=sys.stderr)
        return 1
    print("DRILL OK")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
