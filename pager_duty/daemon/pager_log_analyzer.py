# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
Chrooted Log Analyzer Daemon
----------------------------
Runs continuously as root, immediately drops capabilities and chroots to /var/log,
then assumes the identity of nobody:adm.
Tails system logs for regex anomalies and services Splunk-like UI queries via Redis.
"""

import os
import sys
import time
import re
import json
import logging
import concurrent.futures
import ctypes
import pwd
import grp

import redis

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - [LOG_ANALYZER] - %(message)s"
)
logger = logging.getLogger("pager_log_analyzer")

# --- 1. Pre-Chroot Initialization ---
config_path = os.path.join(os.path.dirname(__file__), "pager_config.json")
if not os.path.exists(config_path):
    logger.critical("pager_config.json not found.")
    sys.exit(1)

try:
    with open(config_path, "r", encoding="utf-8") as f:
        config = json.load(f)
except Exception as e:  # audit-ignore-catch-all
    logger.critical(f"Failed to parse JSON config: {e}")
    sys.exit(1)

log_config = config.get("log_analyzer", {})
target_files = log_config.get("files", [])
patterns = log_config.get("patterns", [])

# Provide default safety patterns if config is empty
if not patterns:
    patterns.append(
        {
            "name": "Kernel Filesystem Corruption",
            "regex": "(?i)(ext4|xfs|btrfs|fs error|corrupt)",
            "severity": "critical",
        }
    )

redis_host = os.getenv("REDIS_HOST") or "redis"
redis_port = int(os.getenv("REDIS_PORT") or "6379")

try:
    r_client = redis.Redis(
        host=redis_host, port=redis_port, db=0, decode_responses=True
    )
    r_client.ping()
    logger.info("Connected to Redis successfully.")
except Exception as e:  # audit-ignore-catch-all
    logger.critical(f"Redis connection failed: {e}")
    sys.exit(1)

# --- 2. Isolation & Privilege Dropping ---
if os.geteuid() == 0:
    logger.info("Executing isolation sequence...")

    # A. Chroot to /var/log
    # Assume POSIX environment with os.chroot available
    if not os.path.exists("/var/log"):
        logger.critical("/var/log missing. Cannot chroot.")
        sys.exit(1)
    os.chdir("/var/log")
    os.chroot("/var/log")
    logger.info("Successfully chrooted to /var/log")

    # B. Drop Kernel Capabilities (PR_CAPBSET_DROP = 24)
    try:
        libc = ctypes.CDLL("libc.so.6")
        for cap in range(40):
            libc.prctl(24, cap, 0, 0, 0)
        logger.info("All kernel bounding capabilities successfully dropped.")
    except Exception as e:  # audit-ignore-catch-all
        logger.warning(f"Could not drop bounding capabilities: {e}")

    # C. Drop to nobody:adm
    try:
        uid = pwd.getpwnam("nobody").pw_uid
        gid = grp.getgrnam("adm").gr_gid
        os.setgroups([])
        os.setresgid(gid, gid, gid)
        os.setresuid(uid, uid, uid)
        logger.info("Privileges successfully de-escalated to nobody:adm")
    except Exception as e:  # audit-ignore-catch-all
        logger.warning(f"Could not setuid to nobody:adm: {e}")


# --- 3. Translation Layer ---
def translate_path(fp):
    """Maps absolute paths to the chrooted filesystem view."""
    # Because we chrooted to /var/log, /var/log/syslog is now just /syslog
    return fp.replace("/var/log", "")


# --- 4. Tailing Engine ---
def tail_file(fp, compiled_patterns):
    chroot_path = translate_path(fp)
    if not chroot_path.startswith("/"):
        chroot_path = "/" + chroot_path

    logger.info(
        f"Starting tail on {chroot_path} for {len(compiled_patterns)} patterns."
    )

    cur_inode = None
    f = None
    while True:
        try:
            try:
                st = os.stat(chroot_path)
                new_inode = st.st_ino
            except FileNotFoundError:
                new_inode = None

            if new_inode != cur_inode:
                if f:
                    f.close()
                if new_inode is not None:
                    try:
                        f = open(chroot_path, "r")
                        if cur_inode is not None:
                            f.seek(0, 0)
                        else:
                            f.seek(0, 2)
                        cur_inode = new_inode
                        logger.info(
                            f"Tailing log file {chroot_path} (inode: {cur_inode})"
                        )
                    except IOError as e:
                        logger.error(f"Failed to open {chroot_path}: {e}")
                        f = None
                        cur_inode = None
                else:
                    f = None
                    cur_inode = None
                    logger.debug(f"Log file {chroot_path} missing.")

            if f:
                line = f.readline()
                if not line:
                    time.sleep(0.5)  # audit-ignore-sleep
                    continue

                for pat in compiled_patterns:
                    if len(pat["c_reg"].findall(line)) > 0:
                        payload = {
                            "source": f"Log Analyzer: {pat['name']}",
                            "severity": pat["severity"],
                            "description": line.strip(),
                            "website_id": pat.get("website_id"),
                        }
                        # Send to generalized monitor via Redis queue to proxy RPC safely
                        r_client.lpush("pager_log_anomalies", json.dumps(payload))
            else:
                time.sleep(5)  # audit-ignore-sleep
        except Exception as e:  # audit-ignore-catch-all
            logger.error(f"Error tailing {chroot_path}: {e}")
            time.sleep(5)  # audit-ignore-sleep


# --- 5. Interactive Splunk UI Listener ---
def redis_search_listener():
    pubsub = r_client.pubsub()
    pubsub.subscribe("log_search_req")
    logger.info("Interactive search listener ready.")

    for message in pubsub.listen():
        if message["type"] == "message":
            try:
                req = json.loads(message["data"])
                uuid_str = req["uuid"]
                fp = req["file"]
                regex = req["regex"]

                chroot_path = translate_path(fp)
                if not chroot_path.startswith("/"):
                    chroot_path = "/" + chroot_path

                matches = []
                c_reg = re.compile(regex, re.IGNORECASE)

                # Perform a reverse-read or simple full scan (capped at 500 matches)
                if os.path.exists(chroot_path):
                    with open(chroot_path, "r") as f:
                        for line in f:
                            if len(c_reg.findall(line)) > 0:
                                matches.append(line.strip())
                                if len(matches) > 500:
                                    break

                # Publish results back to the specific request UUID channel
                res_payload = {"matches": matches}
                r_client.publish(f"log_search_res:{uuid_str}", json.dumps(res_payload))
            except Exception as e:  # audit-ignore-catch-all
                logger.error(f"Search failure: {e}")


# --- 6. Execution ---
if __name__ == "__main__":
    if not target_files:
        logger.info("No files configured for log analysis. Exiting.")
        sys.exit(0)

    # Compile regexes once
    compiled = []
    for p in patterns:
        try:
            compiled.append(
                {
                    "name": p.get("name"),
                    "severity": p.get("severity"),
                    "website_id": p.get("website_id"),
                    "c_reg": re.compile(p.get("regex"), re.IGNORECASE),
                }
            )
        except re.error as e:
            logger.error(f"Invalid regex {p.get('regex')}: {e}")

    # Start Tailers
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=len(target_files) + 1)
    for fp in target_files:
        executor.submit(tail_file, fp, compiled)

    # Start Splunk Listener (Blocks main thread)
    redis_search_listener()
