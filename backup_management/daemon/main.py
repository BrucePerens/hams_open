#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
import os
import sys
import json
import time
import shutil
import pika
import subprocess
import urllib.request
import urllib.error
import logging
import shlex
import re

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - [BACKUP_WORKER] - %(message)s"
)
logger = logging.getLogger("backup_worker")

ODOO_HOST = os.environ.get("ODOO_HOST", "odoo")
ODOO_URL = os.environ.get("ODOO_URL", f"http://{ODOO_HOST}:8069").rstrip("/")
ODOO_DB = os.environ.get("DB_NAME", "odoo")
ODOO_USER = "backup_service_internal"
ODOO_PASS = os.environ.get("ODOO_SERVICE_PASSWORD", "")  # Tested by [@ANCHOR: backup_management:COMM_test_backup_worker_real]

RABBITMQ_HOST = os.environ.get("RABBITMQ_HOST", "rabbitmq")
# Matches the RMQ_USER/RMQ_PASS keys infrastructure.py's rabbitmq.env
# actually provisions -- the previous RABBITMQ_USER/RABBITMQ_PASS names
# never matched, so a real deployment's credentials were never read at
# all and this daemon always silently connected as guest/guest.
RMQ_USER = os.environ.get("RMQ_USER")
RMQ_PASS = os.environ.get("RMQ_PASS")  # Tested by [@ANCHOR: backup_management:COMM_test_backup_worker_real]


# [@ANCHOR: backup_management:COMM_require_rabbitmq_credentials]
def _require_rabbitmq_credentials():
    # This daemon executes backup and restore commands with real filesystem
    # access -- it must never silently connect to RabbitMQ as the
    # well-known "guest"/"guest" default. Fail loudly instead, the same
    # way distributed_redis_cache's HMAC secret check refuses to fall
    # back to a hardcoded, publicly-known secret.
    if not RMQ_USER or not RMQ_PASS:
        raise RuntimeError(
            "RMQ_USER and RMQ_PASS must both be set -- refusing to "
            "fall back to the well-known 'guest'/'guest' default "
            "credentials."
        )


class OdooAPIError(Exception):
    """Custom exception for Odoo JSON-2 API failures."""


# [@ANCHOR: backup_management:COMM_strip_endpoint_scheme]
def _strip_endpoint_scheme(endpoint_url):
    """
    Normalize an admin-entered endpoint_url (e.g. from Backblaze B2's own
    docs, which give "https://s3.us-west-002.backblazeb2.com") into the
    bare host kopia's own --endpoint flag requires.

    Verified against the real kopia 0.23.1 binary on this box: a scheme
    prefix makes `kopia repository connect s3 --endpoint=https://...` fail
    outright with "can't connect to storage: unable to create client:
    Endpoint url cannot have fully qualified paths." pgbackrest's own
    --repo1-s3-endpoint tolerates a scheme prefix (verified separately),
    but we strip it there too for consistency between the two engines.

    Returns (host, disable_tls) -- disable_tls is True only for an
    explicit "http://" scheme, since kopia defaults to TLS and pgbackrest's
    S3 repo driver has no plain-HTTP option at all.
    """
    if not endpoint_url:
        return "", False
    host = endpoint_url.strip()
    disable_tls = False
    if host.startswith("https://"):
        host = host[len("https://") :]
    elif host.startswith("http://"):
        host = host[len("http://") :]
        disable_tls = True
    return host.rstrip("/"), disable_tls


# Where per-backup.config kopia repository-connection state (repository.config
# files) is kept. One file per config_id -- NOT kopia's single global default
# config path -- because this worker serves many backup.config records, each
# potentially pointing at a different S3/B2 bucket with different
# credentials; sharing one global kopia config would make every job silently
# operate against whichever bucket a *previous* job last connected to.
_KOPIA_CONFIG_DIR = os.environ.get(
    "BACKUP_WORKER_KOPIA_CONFIG_DIR", "/var/lib/odoo/backups/.kopia-configs"
)


def _kopia_config_file(config_id):
    return os.path.join(_KOPIA_CONFIG_DIR, f"config_{config_id}.config")


# [@ANCHOR: backup_management:COMM_ensure_kopia_s3_repository]
def _ensure_kopia_s3_repository(config, env_vars, config_file):
    """
    Ensure a kopia repository is connected (in config_file) for this
    backup.config's S3/B2 bucket, creating it on the very first job run
    against that bucket.

    kopia has no single "connect-or-create" command, and the two don't
    overlap: `repository connect` fails against a bucket with no
    repository yet ("repository not initialized in the provided
    storage"), and `repository create` fails against a bucket that
    already has one ("found existing data in storage location") --
    verified empirically against a real local kopia 0.23.1 binary
    (filesystem backend, same repository-init code path as s3/b2) on this
    box; see daemon/test_main.py's TestKopiaEnsureS3Repository for the
    mocked-subprocess assertions built on those exact semantics. So: try
    connect first (the common case on every run after the first); only
    fall back to create when connect fails.

    kopia's own `repository connect b2` subcommand is explicitly marked
    [DEPRECATED] in this box's installed kopia (0.23.1 --help output) --
    B2 is connected here via its S3-compatible API instead (the
    task/finding's own second option), identically to any other non-AWS
    S3-compatible endpoint_url.
    """
    endpoint_host, disable_tls = _strip_endpoint_scheme(config.get("endpoint_url"))
    bucket = config.get("bucket_name") or ""
    base_args = ["--bucket", bucket]
    if endpoint_host:
        base_args += ["--endpoint", endpoint_host]
    if disable_tls:
        base_args += ["--disable-tls"]

    os.makedirs(os.path.dirname(config_file), exist_ok=True)

    connect_cmd = ["kopia", "repository", "connect", "s3"] + base_args
    connect_result = subprocess.run(
        connect_cmd, capture_output=True, text=True, env=env_vars, check=False
    )
    if connect_result.returncode == 0:
        logger.info("Kopia S3/B2 repository already connected (bucket=%s)", bucket)
        return

    logger.info(
        "Kopia repository connect failed (expected on first run against "
        "bucket=%s): %s",
        bucket,
        connect_result.stderr.strip()[-300:],
    )

    create_cmd = ["kopia", "repository", "create", "s3"] + base_args
    create_result = subprocess.run(
        create_cmd, capture_output=True, text=True, env=env_vars, check=False
    )
    if create_result.returncode != 0:
        # ValueError, not RuntimeError: execute_job's own except tuple
        # (OdooAPIError, subprocess.SubprocessError, OSError, ValueError,
        # PermissionError) is what reports this back to Odoo as a failed
        # job and acks the RabbitMQ message -- it does not catch
        # RuntimeError, which would otherwise escape uncaught.
        raise ValueError(
            f"Failed to connect to or create kopia S3/B2 repository "
            f"(bucket={bucket}): connect error: "
            f"{connect_result.stderr.strip()[-300:]!r}; create error: "
            f"{create_result.stderr.strip()[-300:]!r}"
        )
    logger.info("Kopia S3/B2 repository created (bucket=%s)", bucket)


# [@ANCHOR: backup_management:COMM_pgbackrest_s3_repo_args]
def _pgbackrest_s3_repo_args(config, target_path):
    """
    Real, non-secret pgbackrest S3/B2-repo CLI flags for this
    backup.config. access_key/secret_key are deliberately NOT built into
    this argv: pgbackrest's own CLI refuses --repo1-s3-key and
    --repo1-s3-key-secret outright ("ERROR: option 'repo1-s3-key' is not
    allowed on the command-line -- HINT: this option could expose secrets
    in the process list"), verified against the real pgbackrest 2.59.1
    binary on this box. They're threaded through PGBACKREST_REPO1_S3_KEY /
    PGBACKREST_REPO1_S3_KEY_SECRET env vars instead (also verified against
    the real binary), matching this file's existing KOPIA_PASSWORD
    env-var pattern for the same reason: keeping secrets out of the
    `logger.info("Executing: %s", ...)` argv log line below.

    storage_type == "b2" also lands here: pgbackrest has no native B2 repo
    type, so B2 is addressed via its S3-compatible API and endpoint_url,
    same as kopia.
    """
    endpoint_host, _disable_tls = _strip_endpoint_scheme(config.get("endpoint_url"))
    args = [
        "--repo1-type=s3",
        f"--repo1-s3-bucket={config.get('bucket_name') or ''}",
    ]
    if endpoint_host:
        args.append(f"--repo1-s3-endpoint={endpoint_host}")
    # backup.config has no dedicated "region" field (only
    # storage_type/bucket_name/endpoint_url/access_key/secret_key) even
    # though pgbackrest's s3 repo type requires *some* region value.
    # "us-east-1" is the common S3-compatible-provider placeholder and
    # works for AWS S3 and for B2 (B2's S3-compatible API does not
    # validate SigV4 region against the endpoint host). A provider that
    # does enforce region/endpoint agreement would need a real `region`
    # field added to the model -- not done here; see the daemon's own
    # to-do note for this gap.
    args.append(f"--repo1-s3-region={config.get('region') or 'us-east-1'}")
    # No dedicated bucket-prefix/path field exists either; keying the
    # in-bucket path off target_path (the pgbackrest stanza name, already
    # validated elsewhere to ^[a-zA-Z0-9_]+$) keeps configs that share one
    # bucket from colliding, without inventing a new payload field.
    args.append(f"--repo1-path=/{target_path}")
    return args


# Keys inside a job payload dict that carry live, decrypted credentials
# (kopia_password, secret_key, access_key) -- _publish_to_worker in
# backup_config.py puts these on the wire in plaintext for the worker to
# consume, so they must never be written to this daemon's own log file.
_PAYLOAD_SECRET_KEYS = ("kopia_password", "secret_key", "access_key")


def _redact_payload(payload):
    # [@ANCHOR: backup_management:COMM_redact_payload]
    #
    # Bug-hunt fix (2026-09-09, tier-1 pass): execute_job()'s "missing
    # job_id or engine" branch used to log the raw payload dict with
    # `logger.error(..., payload)` -- but that payload is exactly the flat
    # dict _publish_to_worker() builds, which carries kopia_password,
    # secret_key, and access_key in plaintext (see the "config = payload"
    # comment above). Any malformed/incomplete message landing in this
    # branch would write live backup-storage and repository credentials
    # straight into this daemon's own log file. Redact before logging
    # anything that might be this shape.
    if not isinstance(payload, dict):
        return payload
    return {
        k: ("***REDACTED***" if k in _PAYLOAD_SECRET_KEYS and v else v)
        for k, v in payload.items()
    }


# [@ANCHOR: backup_management:COMM_json2_call]
def _json2_call(model, method_name, svc_uid=None, **kwargs):
    payload_str = json.dumps(kwargs)

    headers = {
        "X-Odoo-Database": ODOO_DB,
        "Content-Type": "application/json",
        "Authorization": f"Bearer {ODOO_PASS}",
    }
    if svc_uid:
        headers["X-Odoo-Service-Uid"] = str(svc_uid)
    req = urllib.request.Request(
        f"{ODOO_URL}/json/2/{model}/{method_name}",
        data=payload_str.encode("utf-8"),
        headers=headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            if isinstance(res_data, dict) and res_data.get("error"):
                raise OdooAPIError(f"Odoo Error: {res_data['error']}")
            return res_data
    except urllib.error.HTTPError as e:
        try:
            err_body = e.read().decode("utf-8")
        except UnicodeDecodeError as exc:
            logger.exception("Could not decode response body: %s", exc)
            err_body = "Could not decode response body."
        raise OdooAPIError(f"JSON-2 API HTTP Error {e.code}: {err_body}") from e
    except (urllib.error.URLError, json.JSONDecodeError) as e:
        raise OdooAPIError(f"JSON-2 API Connection/Parse Error: {e}")


# [@ANCHOR: backup_management:COMM_test_backup_worker_real]
def execute_job(ch, method, properties, body):
    try:
        try:
            payload = json.loads(body)
        except json.JSONDecodeError as e:
            decode_err_msg = """Failed to decode RabbitMQ message body: %s"""
            logger.error(decode_err_msg, e)
            ch.basic_ack(delivery_tag=method.delivery_tag)
            return

        job_id = payload.get("job_id")
        engine = payload.get("engine")
        target_path = payload.get("target_path")
        config_id = payload.get("config_id")
        svc_uid = payload.get("svc_uid")
        website_id = payload.get("website_id")

        if not job_id or not engine:
            logger.error(
                "Missing job_id or engine in payload: %s", _redact_payload(payload)
            )
            ch.basic_ack(delivery_tag=method.delivery_tag)
            return

        logger.info("Processing job %s (%s)", job_id, engine)

        _json2_call(
            "backup.job",
            "write",
            svc_uid=svc_uid,
            ids=[job_id],
            vals={
                "state": "processing",
                "output_log": f"Starting {engine} backup...\n",
            },
        )

        # The producer (_publish_to_worker in backup_config.py) sends every
        # field flat at the top level -- there's no nested "config" key and
        # never has been, so `payload.get("config", {})` always evaluated to
        # {}. That silently defeated KOPIA_PASSWORD, made retention settings
        # always fall back to their hardcoded defaults regardless of admin
        # config, and made sync_snapshots always take the pgbackrest branch.
        # The payload already *is* the config; use it directly.
        config = payload

        cmd = []
        if engine == "kopia":
            cmd = ["kopia", "snapshot", "create", "--json", "--", target_path]
        elif engine == "pgbackrest":
            cmd = ["pgbackrest", "backup", f"--stanza={target_path}", "--type=full"]
            if config.get("storage_type") in ("s3", "b2"):
                cmd.extend(_pgbackrest_s3_repo_args(config, target_path))
            keep_daily = config.get("keep_daily", 0)
            if keep_daily > 0:
                cmd.append(f"--repo1-retention-full={keep_daily}")

        elif engine == "kopia_policy":
            keep_daily = config.get("keep_daily", 7)
            keep_weekly = config.get("keep_weekly", 4)
            keep_monthly = config.get("keep_monthly", 6)
            exclude_patterns = config.get("exclude_patterns", "")

            cmd = ["kopia", "policy", "set", "--", target_path]
            cmd.extend(
                [
                    f"--keep-latest={keep_daily}",
                    f"--keep-daily={keep_daily}",
                    f"--keep-weekly={keep_weekly}",
                    f"--keep-monthly={keep_monthly}",
                ]
            )
            if exclude_patterns:
                for line in exclude_patterns.splitlines():
                    if line.strip():
                        cmd.append(f"--add-ignore={line.strip()}")
        elif engine == "sync_snapshots":
            if config.get("config_engine") == "kopia":
                cmd = ["kopia", "snapshot", "list", "--json"]
            else:
                cmd = ["pgbackrest", "info", f"--stanza={target_path}", "--output=json"]
                if config.get("storage_type") in ("s3", "b2"):
                    cmd.extend(_pgbackrest_s3_repo_args(config, target_path))
        elif engine == "restore_drill":
            script_path = payload.get("script")
            allowed_base = os.environ.get("BACKUP_WORKER_SCRIPTS_DIR", "/opt/hams/daemons/backup_worker/scripts")
            try:
                abs_script_path = os.path.realpath(os.path.normpath(script_path)) if script_path else ""
            except OSError:
                abs_script_path = os.path.abspath(script_path) if script_path else ""
            if (
                script_path
                and ".." not in script_path.split(os.path.sep)
                and abs_script_path.startswith(allowed_base + "/")
                and os.path.exists(script_path)
                and os.access(script_path, os.X_OK)
                and script_path.endswith(".py")
            ):
                cmd = [script_path]
            else:
                error_msg = (
                    """Invalid or missing restore drill script: """
                    f"""{script_path}. Must be a .py script."""
                )
                raise ValueError(error_msg)
        elif engine == "restore_cmd":
            cmd = payload.get("cmd_args", [])
            # Security hardening: ensure cmd is a list and contains only allowed binaries
            allowed_binaries = ["kopia", "pgbackrest"]
            if (
                not cmd
                or not isinstance(cmd, list)
                or cmd[0] not in allowed_binaries
            ):
                raise PermissionError(f"Unauthorized command execution attempt: {cmd}")

            if cmd[0] == "kopia":
                # Ensure we don't accidentally write where we shouldn't
                # Kopia restore usually takes a target path as the last argument
                # Odoo-side validation already checks this, but we reinforce here.
                # Expected: ['kopia', 'restore', <snap_id>, <target_path>]
                if len(cmd) != 4 or cmd[1] != "restore":
                    raise ValueError(f"Invalid arguments for kopia restore: {cmd}")
                if cmd[2].startswith("-"):
                    raise PermissionError(f"Malicious snap_id detected: {cmd[2]}")
                target_path_arg = cmd[-1]
                # Re-validate the path in the worker context
                if (
                    target_path_arg.startswith("-")
                    or ".." in target_path_arg
                    or any(c in target_path_arg for c in "; &|`$()<>*?[]{\n'\"")
                ):
                    err_msg = f"""Malicious path detected in worker: {target_path_arg}"""
                    raise PermissionError(err_msg)
                
                try:
                    abs_path = os.path.realpath(os.path.normpath(target_path_arg))
                except OSError:
                    abs_path = os.path.abspath(target_path_arg)
                allowed_restore_base = "/var/lib/odoo/backups"
                if not (abs_path == allowed_restore_base or abs_path.startswith(allowed_restore_base + "/")):
                    err_msg = f"""Malicious path outside allowed base directory: {abs_path}"""
                    raise PermissionError(err_msg)

            elif cmd[0] == "pgbackrest":
                # Expected: ['pgbackrest', 'restore', '--stanza=...', '--set=...']
                # Ensure no dangerous flags are injected
                if len(cmd) < 2 or cmd[1] != "restore":
                    raise ValueError(f"Invalid pgbackrest command: {cmd}")
                for arg in cmd[2:]:
                    if not arg.startswith("--") or "=" not in arg:
                        raise PermissionError(f"Invalid argument format in worker: {arg}")
                    key, val = arg.split("=", 1)
                    if key == "--stanza" and not re.match(r"^[a-zA-Z0-9_]+$", val):
                        raise PermissionError(f"Invalid stanza name in worker: {val}")
                    elif not re.match(r"^[a-zA-Z0-9_.:-]+$", val):
                        err_msg = f"""Malicious argument detected in worker: {arg}"""
                        raise PermissionError(err_msg)

        if not cmd:
            raise ValueError(f"No command generated for engine: {engine}")

        # Keyed off the actual command being run (cmd[0] == "kopia"), not the
        # outer routing "engine" value -- that's what let every restore of a
        # password-protected Kopia snapshot run with no password: restores
        # use engine == "restore_cmd", which never matched the old
        # `engine == "kopia"` check, even when cmd[0] genuinely was "kopia".
        # This also correctly covers kopia_policy and sync_snapshots kopia
        # invocations, which had the identical gap.
        env_vars = os.environ.copy()
        storage_type = config.get("storage_type") or "local"
        if cmd[0] == "kopia":
            if config.get("kopia_password"):
                env_vars["KOPIA_PASSWORD"] = config["kopia_password"]
            if storage_type in ("s3", "b2"):
                # AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY are kopia's own
                # documented env-var overrides for --access-key /
                # --secret-access-key (confirmed via `kopia repository
                # connect s3 --help` on this box) -- used instead of the
                # CLI flags so the credentials never land in the
                # `logger.info("Executing: %s", ...)` argv log line below.
                if config.get("access_key"):
                    env_vars["AWS_ACCESS_KEY_ID"] = config["access_key"]
                if config.get("secret_key"):
                    env_vars["AWS_SECRET_ACCESS_KEY"] = config["secret_key"]
                # KOPIA_CONFIG_PATH (kopia's own env-var form of
                # --config-file, confirmed via --help and by direct testing
                # against a real repository.config on this box) routes
                # every kopia invocation below -- snapshot create, policy
                # set, snapshot list, restore -- at this backup.config's own
                # per-config repository state, not kopia's single global
                # default config.
                config_file = _kopia_config_file(config_id)
                env_vars["KOPIA_CONFIG_PATH"] = config_file
                _ensure_kopia_s3_repository(config, env_vars, config_file)
        elif cmd[0] == "pgbackrest" and storage_type in ("s3", "b2"):
            # See _pgbackrest_s3_repo_args' own docstring: pgbackrest's CLI
            # refuses these two options as command-line flags outright.
            if config.get("access_key"):
                env_vars["PGBACKREST_REPO1_S3_KEY"] = config["access_key"]
            if config.get("secret_key"):
                env_vars["PGBACKREST_REPO1_S3_KEY_SECRET"] = config["secret_key"]

        if not shutil.which(cmd[0]):
            warn_msg = f"""Required binary {cmd[0]} not found. JIT Binary Self-Healing should fetch it here."""
            logger.warning(warn_msg)
            err_msg = f"""Binary {cmd[0]} not found in PATH."""
            raise OSError(err_msg)

        logger.info("Executing: %s", " ".join(shlex.quote(c) for c in cmd))

        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            env=env_vars,
            shell=False,
        )

        log_buffer = ""
        unsent_buffer = ""
        last_update = time.time()

        while True:
            chunk = proc.stdout.read(4096)
            if not chunk:
                break
            log_buffer += chunk
            unsent_buffer += chunk
            # Throttle updates to Odoo to avoid overwhelming it
            if time.time() - last_update > 2.0 and unsent_buffer:
                try:
                    _json2_call(
                        "backup.job",
                        "append_log",
                        svc_uid=svc_uid,
                        ids=[job_id],
                        text_chunk=unsent_buffer,
                    )
                    unsent_buffer = ""
                except urllib.error.URLError as e:
                    logger.warning("Throttled log update failed: %s", e)
                last_update = time.time()

        proc.stdout.close()
        return_code = proc.wait()

        final_state = "done" if return_code == 0 else "failed"
        log_buffer += f"\nProcess exited with code {return_code}"
        unsent_buffer += f"\nProcess exited with code {return_code}"

        # Write final state and send any remaining buffer
        _json2_call(
            "backup.job",
            "write",
            svc_uid=svc_uid,
            ids=[job_id],
            vals={"state": final_state},
        )
        if unsent_buffer:
            _json2_call(
                "backup.job",
                "append_log",
                svc_uid=svc_uid,
                ids=[job_id],
                text_chunk=unsent_buffer,
            )

        if final_state == "done":
            if engine in ("kopia", "pgbackrest", "restore_cmd"):
                _json2_call(
                    "backup.config",
                    "action_sync_snapshots",
                    svc_uid=svc_uid,
                    ids=[config_id],
                )
            elif engine == "sync_snapshots":
                try:
                    # Clean the buffer of the exit message before parsing JSON
                    parts = log_buffer.split("\nProcess exited")
                    json_str = parts[0].strip()

                    match = re.search(r'(\[.*\]|\{.*\})', json_str, re.DOTALL)
                    if match:
                        json_str = match.group(1)

                    data = json.loads(json_str)
                    _json2_call(
                        "backup.config",
                        "action_process_snapshot_data",
                        svc_uid=svc_uid,
                        ids=[config_id],
                        data=data,
                        engine=config.get("config_engine"),
                    )
                except (json.JSONDecodeError, KeyError, ValueError) as e:
                    sync_err_msg = """Failed to parse sync data for engine %s: %s"""
                    logger.error(sync_err_msg, engine, e)
                    _json2_call(
                        "backup.config",
                        "report_backup_failure",
                        svc_uid=svc_uid,
                        ids=[config_id],
                        message=f"Sync Parse Error: {e}",
                    )
            elif engine == "restore_drill":
                _json2_call(
                    "backup.config",
                    "write",
                    svc_uid=svc_uid,
                    ids=[config_id],
                    vals={"last_drill_time": time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime())},
                )

        else:
            error_msg = (
                f"{engine.capitalize()} failed for job {job_id}: {log_buffer[-500:]}"
            )
            _json2_call(
                "backup.config",
                "report_backup_failure",
                svc_uid=svc_uid,
                ids=[config_id],
                message=error_msg,
            )

        ch.basic_ack(delivery_tag=method.delivery_tag)
        logger.info("Job %s finished: %s", job_id, final_state)

    except MemoryError:
        # Genuinely fatal: the process cannot be trusted to keep consuming.
        # Fail fast and let the supervisor restart it.
        raise
    except Exception as e:  # audit-ignore-catch-all: one job's unanticipated failure must fail that job only, not the consuming daemon; MemoryError is re-raised above  # fmt: skip
        # Any other exception, expected (OdooAPIError, OSError, ...) or not
        # (KeyError, AttributeError from a future edit), fails only THIS job.
        # It is logged with its traceback and job context, reported to Odoo
        # below, and the message is acked so the worker keeps consuming.
        logger.exception(
            "Error processing job %s: %s: %s",
            job_id if "job_id" in locals() else "unknown",
            type(e).__name__,
            e,
        )
        # If possible, report the failure back to Odoo before acking
        try:
            payload = json.loads(body)
            job_id = payload.get("job_id")
            config_id = payload.get("config_id")
            svc_uid = payload.get("svc_uid")
            if job_id:
                _json2_call(
                    "backup.job",
                    "write",
                    svc_uid=svc_uid,
                    ids=[job_id],
                    vals={"state": "failed"},
                )
                _json2_call(
                    "backup.job",
                    "append_log",
                    svc_uid=svc_uid,
                    ids=[job_id],
                    text_chunk=f"\nWorker Error: {e}",
                )
            if config_id:
                err_msg = f"""Worker Error ({type(e).__name__}): {e}"""
                _json2_call(
                    "backup.config",
                    "report_backup_failure",
                    svc_uid=svc_uid,
                    ids=[config_id],
                    message=err_msg,
                )
        except Exception as inner_e:  # audit-ignore-catch-all: failure-reporting must never kill the consuming daemon  # fmt: skip
            # Reporting must never kill the daemon either (e.g. a body that
            # is valid JSON but not an object raises AttributeError here).
            report_err_msg = """Failed to report failure back to Odoo: %s"""
            logger.exception(report_err_msg, inner_e)
        ch.basic_ack(delivery_tag=method.delivery_tag)


def main():
    _require_rabbitmq_credentials()
    while True:
        try:
            credentials = pika.PlainCredentials(RMQ_USER, RMQ_PASS)
            parameters = pika.ConnectionParameters(
                host=RABBITMQ_HOST, credentials=credentials
            )
            connection = pika.BlockingConnection(parameters)
            channel = connection.channel()

            channel.queue_declare(queue="backup_tasks", durable=True)
            channel.basic_qos(prefetch_count=1)
            channel.basic_consume(queue="backup_tasks", on_message_callback=execute_job)

            info_msg = """Connected to RABBITMQ. Waiting for backup tasks..."""
            logger.info(info_msg)
            channel.start_consuming()
        except pika.exceptions.AMQPConnectionError:
            logger.warning("RabbitMQ offline. Retrying in 5s...")
            time.sleep(5)  # audit-ignore-sleep: [@ANCHOR: backup_management:COMM_audit_ignore_sleep_1]
        except pika.exceptions.AMQPError as e:
            proto_err_msg = """RabbitMQ protocol error: %s. Restarting..."""
            logger.error(proto_err_msg, e)
            time.sleep(5)  # audit-ignore-sleep: [@ANCHOR: backup_management:COMM_audit_ignore_sleep_3]
        except OdooAPIError as e:
            err_msg = """Fatal Odoo API error in main loop: %s. Retrying in 10s..."""
            logger.error(err_msg, e)
            time.sleep(10)  # audit-ignore-sleep: [@ANCHOR: backup_management:COMM_audit_ignore_sleep_2]
        except (
            ValueError,
            TypeError,
            OSError,
        ) as e:
            err_msg = """Unexpected error in main loop: %s. Restarting..."""
            logger.exception(err_msg, e)
            time.sleep(5)  # audit-ignore-sleep: [@ANCHOR: backup_management:COMM_audit_ignore_sleep_4]


if __name__ == "__main__":
    if "--start-test" in sys.argv:
        try:
            _require_rabbitmq_credentials()
            credentials = pika.PlainCredentials(RMQ_USER, RMQ_PASS)
            parameters = pika.ConnectionParameters(
                host=RABBITMQ_HOST, credentials=credentials
            )
            conn = pika.BlockingConnection(parameters)
            conn.close()
            logger.info("Smoketest passed. Exiting successfully.")
            sys.exit(0)
        except (RuntimeError, pika.exceptions.AMQPError, OSError) as e:
            logger.error("Smoketest failed: %s", e)
            sys.exit(1)

    main()
