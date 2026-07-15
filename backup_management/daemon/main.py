#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
import os
import json
import time
import hmac
import hashlib
import secrets
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

ODOO_HOST = os.environ.get("ODOO_HOST", "localhost")
ODOO_URL = os.environ.get("ODOO_URL", f"http://{ODOO_HOST}:8069").rstrip("/")
ODOO_DB = os.environ.get("DB_NAME", "odoo")
ODOO_USER = "backup_service_internal"
ODOO_PASS = os.environ.get("ODOO_SERVICE_PASSWORD", "")  # burn-ignore-env: # Tested by [@ANCHOR: backup_management:COMM_test_backup_worker_real]

RABBITMQ_HOST = os.environ.get("RABBITMQ_HOST", "localhost")
RABBITMQ_USER = os.environ.get("RABBITMQ_USER", "guest")
RABBITMQ_PASS = os.environ.get("RABBITMQ_PASS", "guest")  # burn-ignore-env: # Tested by [@ANCHOR: backup_management:COMM_test_backup_worker_real]


class OdooAPIError(Exception):
    """Custom exception for Odoo JSON-2 API failures."""


def _json2_call(model, method_name, svc_uid=None, **kwargs):
    timestamp = str(int(time.time()))
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
            logger.error("Missing job_id or engine in payload: %s", payload)
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

        config = payload.get("config", {})

        env_vars = os.environ.copy()
        if engine == "kopia" and config.get("kopia_password"):
            env_vars["KOPIA_PASSWORD"] = config["kopia_password"]

        cmd = []
        if engine == "kopia":
            cmd = ["kopia", "snapshot", "create", "--json", "--", target_path]
        elif engine == "pgbackrest":
            cmd = ["pgbackrest", "backup", f"--stanza={target_path}", "--type=full"]
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
            if config.get("engine") == "kopia":
                cmd = ["kopia", "snapshot", "list", "--json"]
            else:
                cmd = ["pgbackrest", "info", f"--stanza={target_path}", "--output=json"]
        elif engine == "restore_drill":
            script_path = payload.get("script")
            allowed_base = os.environ.get("BACKUP_WORKER_SCRIPTS_DIR", "/opt/odoo/daemons/backup_worker/scripts")
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
                    or any(c in target_path_arg for c in "; &|`$()<>*?[]{\n")
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
                        engine=config.get("engine"),
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

    except (
        OdooAPIError,
        subprocess.SubprocessError,
        OSError,
        ValueError,
        PermissionError,
    ) as e:
        logger.error(
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
        except (
            OdooAPIError,
            json.JSONDecodeError,
            urllib.error.URLError,
        ) as inner_e:
            report_err_msg = """Failed to report failure back to Odoo: %s"""
            logger.exception(report_err_msg, inner_e)
        ch.basic_ack(delivery_tag=method.delivery_tag)


def main():
    while True:
        try:
            credentials = pika.PlainCredentials(RABBITMQ_USER, RABBITMQ_PASS)
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
    main()
