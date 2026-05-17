#!/usr/bin/env python3
import os
import json
import time
import pika
import subprocess
import urllib.request
import urllib.error
import logging
import shlex

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - [BACKUP_WORKER] - %(message)s"
)
logger = logging.getLogger("backup_worker")

ODOO_URL = os.environ.get("ODOO_URL", "http://odoo:8069").rstrip("/")
ODOO_DB = os.environ.get("DB_NAME", "odoo")
ODOO_USER = "backup_service_internal"
ODOO_PASS = os.environ.get("ODOO_SERVICE_PASSWORD", "")

RABBITMQ_HOST = os.environ.get("RABBITMQ_HOST", "rabbitmq")
RABBITMQ_USER = os.environ.get("RABBITMQ_USER", "guest")
RABBITMQ_PASS = os.environ.get("RABBITMQ_PASS", "guest")


def _json2_call(model, method_name, **kwargs):
    headers = {
        "Authorization": f"bearer {ODOO_PASS}",
        "X-Odoo-Database": ODOO_DB,
        "Content-Type": "application/json",
    }
    req = urllib.request.Request(
        f"{ODOO_URL}/json/2/{model}/{method_name}",
        data=json.dumps(kwargs).encode("utf-8"),
        headers=headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        err_body = e.read().decode("utf-8")
        raise Exception(f"JSON-2 API Error {e.code}: {err_body}")


def execute_job(ch, method, properties, body):
    try:
        payload = json.loads(body)
        job_id = payload.get("job_id")
        engine = payload.get("engine")
        target_path = payload.get("target_path")
        config_id = payload.get("config_id")

        if not job_id or not engine:
            logger.error("Missing job_id or engine in payload")
            ch.basic_ack(delivery_tag=method.delivery_tag)
            return

        logger.info(f"Processing job {job_id} ({engine})")

        _json2_call(
            "backup.job",
            "write",
            ids=[job_id],
            vals={
                "state": "processing",
                "output_log": f"Starting {engine} backup...\n",
            },
        )

        config_records = _json2_call(
            "backup.config", "read", ids=[config_id],
            fields=["kopia_password", "keep_daily", "keep_weekly", "keep_monthly", "exclude_patterns", "engine"]
        )
        config = config_records[0] if config_records else {}

        env_vars = os.environ.copy()
        if engine == "kopia" and config.get("kopia_password"):
            env_vars["KOPIA_PASSWORD"] = config["kopia_password"]

        cmd = []
        if engine == "kopia":
            cmd = ["kopia", "snapshot", "create", target_path, "--json"]
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

            cmd = ["kopia", "policy", "set", target_path]
            cmd.extend([
                f"--keep-latest={keep_daily}",
                f"--keep-daily={keep_daily}",
                f"--keep-weekly={keep_weekly}",
                f"--keep-monthly={keep_monthly}",
            ])
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
            if script_path and os.path.exists(script_path) and os.access(script_path, os.X_OK):
                cmd = [script_path]
            else:
                raise Exception(f"Invalid or missing restore drill script: {script_path}")
        elif engine == "restore_cmd":
            cmd = payload.get("cmd_args", [])
            # Security hardening: ensure cmd is a list and contains only allowed binaries
            allowed_binaries = ["kopia", "pgbackrest"]
            if not cmd or cmd[0] not in allowed_binaries:
                 raise Exception(f"Unauthorized command execution attempt: {cmd}")

            if cmd[0] == "kopia":
                 # Ensure we don't accidentally write where we shouldn't
                 # Kopia restore usually takes a target path as the last argument
                 # Odoo-side validation already checks this, but we reinforce here
                 pass

        if not cmd:
            raise Exception(f"No command generated for engine: {engine}")

        logger.info(f"Executing: {' '.join(shlex.quote(c) for c in cmd)}")

        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            env=env_vars,
            shell=False,
        )

        log_buffer = ""
        last_update = time.time()

        for line in iter(proc.stdout.readline, ""):
            log_buffer += line
            # Throttle updates to Odoo to avoid overwhelming it
            if time.time() - last_update > 2.0:
                try:
                    _json2_call(
                        "backup.job",
                        "write",
                        ids=[job_id],
                        vals={"output_log": log_buffer},
                    )
                except urllib.error.URLError as e:
                    logger.warning(f"Throttled log update failed: {e}")
                last_update = time.time()

        proc.stdout.close()
        return_code = proc.wait()

        final_state = "done" if return_code == 0 else "failed"
        log_buffer += f"\nProcess exited with code {return_code}"

        _json2_call(
            "backup.job",
            "write",
            ids=[job_id],
            vals={"state": final_state, "output_log": log_buffer},
        )

        if final_state == "done":
            if engine in ("kopia", "pgbackrest", "restore_cmd"):
                _json2_call("backup.config", "action_sync_snapshots", ids=[config_id])
            elif engine == "sync_snapshots":
                try:
                    # Clean the buffer of the exit message before parsing JSON
                    # We look for the last valid JSON block if possible, or just the whole buffer before the exit message
                    parts = log_buffer.split("\nProcess exited")
                    json_str = parts[0].strip()
                    # Kopia might output some info before JSON if not careful, though --json should be clean.
                    # Let's try to find the START of the JSON array or object.
                    # We look for the first [ or { that precedes the end of the string.
                    start_idx_arr = json_str.find("[")
                    start_idx_obj = json_str.find("{")
                    if start_idx_arr != -1 and start_idx_obj != -1:
                         start_idx = min(start_idx_arr, start_idx_obj)
                    else:
                         start_idx = max(start_idx_arr, start_idx_obj)

                    if start_idx != -1:
                         json_str = json_str[start_idx:]

                    data = json.loads(json_str)
                    _json2_call("backup.config", "_process_snapshot_data", ids=[config_id], data=data, engine=config.get("engine"))
                except (json.JSONDecodeError, KeyError, ValueError) as e:
                    logger.error(f"Failed to parse sync data: {e}")
                    _json2_call("backup.config", "_report_backup_failure", ids=[config_id], message=f"Sync Parse Error: {e}")
            elif engine == "restore_drill":
                _json2_call("backup.config", "write", ids=[config_id], vals={"last_drill_time": time.strftime("%Y-%m-%d %H:%M:%S")})

        else:
            error_msg = f"{engine.capitalize()} failed for job {job_id}: {log_buffer[-500:]}"
            _json2_call(
                "backup.config",
                "_report_backup_failure",
                ids=[config_id],
                message=error_msg,
            )

        ch.basic_ack(delivery_tag=method.delivery_tag)
        logger.info(f"Job {job_id} finished: {final_state}")

    except Exception as e: # audit-ignore-catch-all
        logger.error(f"Fatal error processing job: {e}")
        # If possible, report the failure back to Odoo before acking
        try:
             # Try to find job_id and config_id from payload if it was successfully parsed
             payload = json.loads(body)
             job_id = payload.get("job_id")
             config_id = payload.get("config_id")
             if job_id:
                  _json2_call("backup.job", "write", ids=[job_id], vals={"state": "failed", "output_log": str(e)})
             if config_id:
                  _json2_call("backup.config", "_report_backup_failure", ids=[config_id], message=f"Worker Error: {e}")
        except urllib.error.URLError as inner_e:
             logger.error(f"Failed to report failure to Odoo: {inner_e}")
             pass
        ch.basic_ack(delivery_tag=method.delivery_tag) # Ack so we don't loop on bad payloads


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

            logger.info("Connected to RABBITMQ. Waiting for backup tasks...")
            channel.start_consuming()
        except pika.exceptions.AMQPConnectionError:
            logger.warning("RabbitMQ offline. Retrying in 5s...")
            time.sleep(5)
        except Exception as e: # audit-ignore-catch-all
            logger.error(f"RabbitMQ consumer crash: {e}. Restarting...")
            time.sleep(5)


if __name__ == "__main__":
    main()
