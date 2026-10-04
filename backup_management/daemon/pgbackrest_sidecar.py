#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
Privileged sidecar for pgbackrest operations backup_worker's own main.py
cannot perform itself. See main.py's _run_pgbackrest_via_sidecar() for the
full architecture rationale (MASTER_01 ADR section 6, "OS-Level Daemon
Restriction" -- the same airgapped-spooling shape already mandated there
for privileged hardware telemetry, roles reversed: here the UNprivileged
daemon writes the request and this privileged sidecar writes the result).

Started by hams-pgbackrest-backup.path whenever a request file appears in
PGBACKREST_SPOOL_DIR. Runs as root (via its own systemd unit, deliberately
unsandboxed -- see hams-pgbackrest-backup.service's own comment), which can
read backup_worker's 0700 odoo-owned spool directory regardless of mode,
then runuser's down to postgres for the actual pgbackrest invocation --
never runs pgbackrest as root itself. Processes every pending request file
found (not just whichever one happened to trigger this run, in case several
queued up while a previous run was busy), then exits: this is a
Type=oneshot unit, not a long-running daemon, matching
hams.db.local.backup.service's own established shape for exactly this kind
of "privileged, infrequent, filesystem-only" operation.

Tested by [@ANCHOR: backup_management:COMM_test_pgbackrest_sidecar]
"""
import glob
import json
import logging
import os
import re
import subprocess

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - [PGBACKREST_SIDECAR] - %(message)s"
)
logger = logging.getLogger("pgbackrest_sidecar")

SPOOL_DIR = os.environ.get("PGBACKREST_SPOOL_DIR", "/opt/hams/backup_requests")
RUN_AS_USER = os.environ.get("PGBACKREST_SIDECAR_RUN_AS", "postgres")
PGBACKREST_BIN = os.environ.get("PGBACKREST_BIN", "pgbackrest")

_STANZA_RE = re.compile(r"^[a-zA-Z0-9_]+$")
_GENERIC_VALUE_RE = re.compile(r"^[a-zA-Z0-9_.:/-]+$")
_REPO_OPTION_RE = re.compile(r"^--repo=[0-9]{1,3}$")
_REPO_RETENTION_RE = re.compile(r"^--repo[0-9]{1,3}-retention-full=[0-9]{1,5}$")
# Operations delegated here. "info" is read-only but must read
# /etc/pgbackrest/pgbackrest.conf, which holds the repository cipher
# passphrase and storage keys and is therefore readable only by root and
# postgres (mode 0640): the unprivileged daemon cannot read it.
_ALLOWED_OPERATIONS = ("backup", "info")

# Every argv prefix this sidecar will ever pass through to a real pgbackrest
# invocation as postgres. Exhaustive, not a blocklist: anything not matched
# here is refused. Mirrors exactly what main.py's own cmd-building code for
# engine == "pgbackrest" (the "backup" and "info" branches) and _pgbackrest_s3_repo_args()
# actually produce -- grow this list only in lockstep with those, never ahead
# of them speculatively.
_ALLOWED_FLAG_PREFIXES = (
    "--repo1-retention-full=",
    "--repo1-s3-bucket=",
    "--repo1-s3-endpoint=",
    "--repo1-s3-region=",
    "--repo1-path=",
)


# [@ANCHOR: backup_management:COMM_pgbackrest_sidecar_validate_request]
# Verified by [@ANCHOR: backup_management:COMM_test_pgbackrest_sidecar_validate_cmd]
def _validate_request_cmd(cmd):
    """Defense in depth: backup_worker already builds and validates this
    argv before handing it off, but this process is about to run it as
    postgres -- never trust the other side of a privilege boundary, the
    same posture main.py's own restore_cmd branch already takes re-checking
    a request it didn't itself originate."""
    if not isinstance(cmd, list) or not cmd:
        raise ValueError(f"Empty or non-list sidecar request cmd: {cmd!r}")
    if cmd[0] != "pgbackrest":
        raise ValueError(f"Refusing non-pgbackrest sidecar request: {cmd!r}")
    if len(cmd) < 2 or cmd[1] not in _ALLOWED_OPERATIONS:
        raise ValueError(f"Refusing unsupported pgbackrest sidecar operation: {cmd!r}")

    stanza_seen = False
    for arg in cmd[2:]:
        if arg.startswith("--stanza="):
            if not _STANZA_RE.match(arg.split("=", 1)[1]):
                raise ValueError(f"Invalid stanza in sidecar request: {arg!r}")
            stanza_seen = True
        elif arg.startswith("--type="):
            if arg.split("=", 1)[1] not in ("full", "diff", "incr"):
                raise ValueError(f"Invalid backup type in sidecar request: {arg!r}")
        elif arg == "--repo1-type=s3":
            pass
        elif arg == "--output=json" and cmd[1] == "info":
            pass
        elif _REPO_OPTION_RE.match(arg) or _REPO_RETENTION_RE.match(arg):
            pass
        elif any(arg.startswith(prefix) for prefix in _ALLOWED_FLAG_PREFIXES):
            value = arg.split("=", 1)[1]
            if not _GENERIC_VALUE_RE.match(value):
                raise ValueError(f"Invalid argument value in sidecar request: {arg!r}")
        else:
            raise ValueError(f"Unrecognized pgbackrest argument in sidecar request: {arg!r}")
    if not stanza_seen:
        raise ValueError(f"Sidecar request has no --stanza: {cmd!r}")


# [@ANCHOR: backup_management:COMM_pgbackrest_sidecar_validate_env]
# Verified by [@ANCHOR: backup_management:COMM_test_pgbackrest_sidecar_validate_env]
def _validate_request_env(env):
    """Only these two secret keys may ride along in a request -- anything
    else (an attempt to inject PATH, LD_PRELOAD, etc. via a compromised or
    buggy request writer) is refused outright rather than silently
    dropped, so a bug on the writing side fails loudly instead of quietly
    running with an unexpected environment."""
    if not isinstance(env, dict):
        raise ValueError(f"Sidecar request env must be a dict: {env!r}")
    allowed = {"PGBACKREST_REPO1_S3_KEY", "PGBACKREST_REPO1_S3_KEY_SECRET"}
    unexpected = set(env) - allowed
    if unexpected:
        raise ValueError(f"Unexpected keys in sidecar request env: {sorted(unexpected)}")
    for value in env.values():
        if not isinstance(value, str):
            raise ValueError("Sidecar request env values must be strings")


# [@ANCHOR: backup_management:COMM_pgbackrest_sidecar_process_one]
# Verified by [@ANCHOR: backup_management:COMM_test_pgbackrest_sidecar_process_one]
# Verified by [@ANCHOR: backup_management:COMM_test_pgbackrest_sidecar_main]
def _process_one(request_path):
    job_id = os.path.basename(request_path)[len("request-") : -len(".json")]
    result_path = os.path.join(SPOOL_DIR, f"result-{job_id}.json")

    try:
        with open(request_path) as f:
            request = json.load(f)
        cmd = request.get("cmd")
        env = request.get("env", {})
        _validate_request_cmd(cmd)
        _validate_request_env(env)

        # Inherit this (otherwise-unsandboxed, EnvironmentFile-free) oneshot
        # unit's own ordinary process environment -- it carries nothing
        # sensitive, since this unit deliberately declares no
        # EnvironmentFile=/Environment= of its own -- and layer in only the
        # two validated secret keys from the request. runuser itself still
        # resets HOME/SHELL/USER/LOGNAME to the postgres account's own
        # passwd entry regardless of what's passed here.
        full_env = os.environ.copy()
        full_env.update(env)
        full_cmd = ["runuser", "-u", RUN_AS_USER, "--", PGBACKREST_BIN] + cmd[1:]
        proc = subprocess.run(
            full_cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            env=full_env,
            check=False,
        )
        result = {"return_code": proc.returncode, "output": proc.stdout}
    except Exception as e:  # audit-ignore-catch-all: one malformed/invalid request must fail only that job, writing a result backup_worker can read, not crash this oneshot run and leave backup_worker polling until its own timeout  # fmt: skip
        logger.exception("Job %s failed", job_id)
        result = {"return_code": 1, "output": f"Sidecar error: {e}"}

    tmp_result_path = result_path + f".tmp-{os.getpid()}"
    try:
        with open(tmp_result_path, "w") as f:
            json.dump(result, f)
        os.rename(tmp_result_path, result_path)
        os.chmod(result_path, 0o644)  # readable back by backup_worker (odoo), which owns the spool dir
        os.remove(request_path)  # shortens the S3/B2 secret's lifetime on disk
    except OSError:
        # Advisor-caught gap, 2026-10-01: a bare, unwrapped os.remove() here
        # meant that if writing/renaming the result itself failed (full
        # disk, an odd permission problem), the request file would still
        # be sitting there afterward -- still matching
        # PathExistsGlob=request-*.json, so hams-pgbackrest-backup.path
        # would re-trigger this service immediately, and again, and again,
        # until systemd's StartLimitBurst trips and leaves the .path unit
        # itself failed -- at which point no future backup request can
        # trigger anything until a human runs `systemctl reset-failed`.
        # Renaming the request out of the glob pattern (rather than
        # deleting it, and rather than leaving it matching) stops that
        # loop while keeping the evidence for whoever investigates. Never
        # re-raises: main() must keep processing any other queued request
        # files in this same run.
        logger.exception("Job %s: failed to write its result or clear its request", job_id)
        if os.path.exists(request_path):
            os.rename(request_path, os.path.join(SPOOL_DIR, f"failed-{job_id}.json"))


def main():
    os.makedirs(SPOOL_DIR, exist_ok=True)
    request_paths = sorted(glob.glob(os.path.join(SPOOL_DIR, "request-*.json")))
    for request_path in request_paths:
        _process_one(request_path)


if __name__ == "__main__":
    main()
