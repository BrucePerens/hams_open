# SPDX-License-Identifier: AGPL-3.0-or-later

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
# Bound at import, before the chroot: concurrent.futures loads its .thread submodule lazily, and inside /var/log there
# is no Python library to load it from (hams1 2026-11-07: ModuleNotFoundError 'concurrent.futures.thread' right after
# the privilege drop).
from concurrent.futures import ThreadPoolExecutor
import ctypes
import pwd
import grp
import collections
import socket
import redis

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - [LOG_ANALYZER] - %(message)s"
)
logger = logging.getLogger("pager_log_analyzer")


# --- 3. Translation Layer ---
# [@ANCHOR: pager_duty:translate_path]
def translate_path(fp):
    """Maps absolute paths to the chrooted filesystem view."""
    # Because we chrooted to /var/log, /var/log/syslog is now just /syslog
    #
    # A plain fp.replace("/var/log", "") would strip
    # every occurrence of the substring anywhere in fp, not just a leading
    # "/var/log" prefix -- e.g. translate_path("/srv/var/log/x") silently
    # returned "/srv/x" instead of leaving the unexpected prefix alone.
    # Not exploitable (both call sites only ever pass config-controlled or
    # already-gated internal paths, per translate_path.md), but a real
    # correctness gap. removeprefix only strips a genuine leading match.
    return fp.removeprefix("/var/log")


# --- 3b. Redis helpers ---
# [@ANCHOR: pager_duty:log_analyzer_resolve_redis_host]
def resolve_redis_host(host):
    """The IP address to connect to Redis with. Called before the chroot, because inside /var/log there is no /etc/hosts,
    resolv.conf or nsswitch.conf, so a later reconnect by the name `redis` would never resolve. Returns `host` unchanged
    when it is already an address or cannot be resolved (the connection attempt then reports the real error)."""
    for family in (socket.AF_INET, socket.AF_UNSPEC):  # IPv4 first: Redis on this host listens on the IPv4 loopback
        try:
            return socket.getaddrinfo(host, None, family=family, type=socket.SOCK_STREAM)[0][4][0]
        except (OSError, IndexError):
            continue
    return host


# [@ANCHOR: pager_duty:log_analyzer_push_anomaly]
def push_anomaly(r_client, payload, retry_seconds=5):
    """Queue one anomaly for the monitor on the Redis list `pager_log_anomalies`, waiting out a Redis outage.

    A match must not be lost, and a Redis error must not kill the tailing thread (an uncaught exception ends the thread
    silently while the daemon looks alive): this retries every `retry_seconds` until the push succeeds. Blocks the calling
    tailer only; the file keeps growing and is read on afterwards."""
    while True:
        try:
            r_client.lpush("pager_log_anomalies", json.dumps(payload))
            return
        except redis.RedisError as e:
            logger.error(f"Could not queue a log anomaly in Redis, retrying: {e}")
            time.sleep(retry_seconds)  # audit-ignore-sleep


# --- 4. Tailing Engine ---
# [@ANCHOR: pager_duty:tail_file]
def tail_file(r_client, fp, compiled_patterns):
    chroot_path = translate_path(fp)
    if not chroot_path.startswith("/"):
        chroot_path = "/" + chroot_path

    logger.info(
        f"Starting tail on {chroot_path} for {len(compiled_patterns)} patterns."
    )

    cur_inode = None
    f = None
    missing_logged = False
    while True:
        try:
            try:
                st = os.stat(chroot_path)
                new_inode = st.st_ino
            except FileNotFoundError:
                st = None
                new_inode = None

            # copytruncate rotation (the Odoo logrotate entry): the inode stays, the file shrinks, and the offset is past the
            # new end, so nothing would be read until the log regrew beyond the old offset.
            if f and st is not None and new_inode == cur_inode and st.st_size < f.tell():
                logger.info(f"Log file {chroot_path} was truncated; reading it from the start.")
                f.seek(0, 0)

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
                        missing_logged = False
                        logger.info(
                            f"Tailing log file {chroot_path} (inode: {cur_inode})"
                        )
                    except IOError as e:
                        logger.error(f"Failed to open {chroot_path}: {e}")
                        f = None
                        cur_inode = None
                else:
                    if cur_inode is not None or not missing_logged:
                        logger.warning(f"Log file {chroot_path} is missing; waiting for it.")
                        missing_logged = True
                    f = None
                    cur_inode = None

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
                        push_anomaly(r_client, payload)
            else:
                time.sleep(5)  # audit-ignore-sleep
        except OSError as e:
            logger.error(f"Error tailing {chroot_path}: {e}")
            time.sleep(5)  # audit-ignore-sleep


# --- 5. Interactive Splunk UI Listener ---
# [@ANCHOR: pager_duty:redis_search_listener]
def redis_search_listener(r_client):
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

                matches_deque = collections.deque(maxlen=500)
                try:
                    c_reg = re.compile(regex, re.IGNORECASE)
                except re.error as e:
                    res_payload = {"error": f"Invalid regex: {e}"}
                    r_client.lpush("pager_log_search_res_queue", json.dumps({"uuid": uuid_str, "payload": res_payload}))
                    continue

                # Perform a simple full scan and keep the 500 most recent matches
                if os.path.exists(chroot_path):
                    with open(chroot_path, "r") as f:
                        for line in f:
                            if len(c_reg.findall(line)) > 0:
                                matches_deque.append(line.strip())

                matches = list(matches_deque)

                # Push results back to queue for Asynchronous Bastion Pattern
                res_payload = {"matches": matches}
                r_client.lpush("pager_log_search_res_queue", json.dumps({"uuid": uuid_str, "payload": res_payload}))
            # Catches more than (OSError, json.JSONDecodeError, KeyError): a
            # message that parses as valid JSON but isn't a
            # dict (e.g. a JSON array/string) makes `req["uuid"]` raise
            # TypeError, not KeyError -- previously uncaught, which crashed this
            # whole listener (and, since it's the only thing blocking main(),
            # the entire chrooted log-analyzer daemon and every one of its
            # tail_file() threads) on a single malformed message instead of
            # just failing that one search request.
            except (OSError, json.JSONDecodeError, KeyError, TypeError) as e:
                logger.error(f"Search failure: {e}")


# --- 6. Execution ---
# Sections 1 (config load) and 2 (Redis connect + chroot/privilege-drop) used
# to run unconditionally at *module import time* -- a real, previously-
# undiscovered hazard found while closing this module's own Stage 1 test-
# anchor gaps: merely `import`ing this file for any reason at all (a linter,
# a REPL, a future test) connected to a real Redis server, chrooted the
# calling process to /var/log, and dropped its privileges to nobody:adm,
# none of which is reversible within the same process. Moved into main(),
# guarded by __name__ == "__main__" below, matching generalized_monitor.py's
# own convention of doing real work only when actually run as a script.
# tail_file()/redis_search_listener() now take r_client as an explicit
# parameter instead of reading a module-global connection, which is what
# makes them callable against a real *test* Redis double at all.
# [@ANCHOR: pager_duty:log_analyzer_resolve_config_path]
def resolve_config_path(environ=None):
    """The configuration file to read, by the same rule as generalized_monitor.resolve_config_path.

    PAGER_CONFIG_PATH (an explicit file), then pager_config.json in PAGER_CONFIG_DIR (default /opt/hams/etc) if it exists,
    then the file next to this script. This daemon used to read only the file next to itself, ignoring PAGER_CONFIG_PATH, so
    the log patterns an administrator pushed from Odoo never reached it. (It cannot import generalized_monitor, which is
    large and starts threads; the two rules are kept equal by a test.)"""
    environ = os.environ if environ is None else environ
    explicit = environ.get("PAGER_CONFIG_PATH")
    if explicit:
        return explicit
    system_path = os.path.join(environ.get("PAGER_CONFIG_DIR", "/opt/hams/etc"), "pager_config.json")
    if os.path.exists(system_path):
        return system_path
    return os.path.join(os.path.dirname(__file__), "pager_config.json")


# [@ANCHOR: pager_duty:log_analyzer_main]
def main():
    config_path = resolve_config_path()
    logger.info(f"Reading configuration from {config_path}")
    if not os.path.exists(config_path):
        logger.critical(f"Configuration file not found at {config_path}.")
        sys.exit(1)

    try:
        with open(config_path, "r", encoding="utf-8") as f:
            config = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
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

    # Resolved here, before the chroot: no name service is reachable from inside /var/log (see resolve_redis_host).
    redis_host = resolve_redis_host(os.getenv("REDIS_HOST") or "redis")
    redis_port = int(os.getenv("REDIS_PORT") or "6379")
    # Real fix, found by an adversarial security review: this used to
    # connect with no password at all, regardless of whether one is
    # configured -- see generalized_monitor.py's own REDIS_PASS comment for
    # the full reasoning (this daemon's log_search_req/log_search_res/
    # log_anomalies channels are the other end of that same trust boundary).
    redis_password = os.getenv("REDIS_PASSWORD") or os.getenv("redis_password")
    redis_username = os.getenv("REDIS_USERNAME") or None  # the production ACL user (redis.env)

    try:
        r_client = redis.Redis(
            host=redis_host, port=redis_port, db=0, username=redis_username, password=redis_password,
            decode_responses=True,
        )
        r_client.ping()
        logger.info("Connected to Redis successfully.")
    except redis.RedisError as e:
        logger.critical(f"Redis connection failed: {e}")
        sys.exit(1)

    # --- 2. Isolation & Privilege Dropping ---
    if os.geteuid() == 0:
        logger.info("Executing isolation sequence...")

        # Resolve UID/GID before chrooting because /etc/passwd won't be accessible
        uid = pwd.getpwnam("nobody").pw_uid
        gid = grp.getgrnam("adm").gr_gid

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
            failed = [cap for cap in range(40) if libc.prctl(24, cap, 0, 0, 0) != 0]
            if failed:
                logger.warning(f"Could not drop {len(failed)} bounding capabilities: {failed[:5]}")
            else:
                logger.info("All kernel bounding capabilities successfully dropped.")
        except OSError as e:
            logger.warning(f"Could not drop bounding capabilities: {e}")

        # C. Drop to nobody:adm
        try:
            os.setgroups([])
            os.setresgid(gid, gid, gid)
            os.setresuid(uid, uid, uid)
            logger.info("Privileges successfully de-escalated to nobody:adm")
        except OSError as e:
            # Still root, inside the chroot: never go on tailing logs with that.
            logger.critical(f"Could not drop to nobody:adm, refusing to run as root: {e}")
            sys.exit(1)

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
    executor = ThreadPoolExecutor(max_workers=len(target_files) + 1)
    for fp in target_files:
        executor.submit(tail_file, r_client, fp, compiled)

    # Start Splunk Listener (Blocks main thread). If it ends or fails, the whole process must end so systemd restarts it:
    # the tailer threads loop forever and would otherwise keep a half-dead daemon alive at interpreter shutdown.
    try:
        redis_search_listener(r_client)
    except Exception as e:  # audit-ignore-catch-all
        logger.critical(f"Search listener failed, exiting so that systemd restarts the daemon: {e}")
    os._exit(1)


if __name__ == "__main__":
    main()
